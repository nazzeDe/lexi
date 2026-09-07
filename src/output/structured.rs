use html5ever::TokenizerResult;
use html5ever::buffer_queue::BufferQueue;
use html5ever::tokenizer::{Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use html5ever::tree_builder::TreeBuilder;
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use std::cell::RefCell;

#[derive(Debug)]
pub(super) enum Block {
    Heading(String),
    Label(String),
    Sense {
        number: String,
        text: String,
    },
    Definition {
        text: String,
        subsense: bool,
        starts_subsense: bool,
    },
    Example {
        lines: Vec<String>,
        first_direct: bool,
        subsense: bool,
    },
    Etymology(Vec<Block>),
    Other(String),
}

// The source uses XML-style empty anchors in otherwise ordinary HTML. Close only
// those anchors before HTML5's adoption-agency algorithm can reparent later text.
struct DictionaryTree(TreeBuilder<Handle, RcDom>);

impl TokenSink for DictionaryTree {
    type Handle = Handle;

    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<Handle> {
        if let Token::TagToken(ref tag) = token
            && tag.kind == TagKind::StartTag
            && tag.self_closing
            && tag.name.as_ref() == "a"
        {
            let mut start = tag.clone();
            start.self_closing = false;
            let end = Tag {
                kind: TagKind::EndTag,
                attrs: Vec::new(),
                ..start.clone()
            };
            let _ = self.0.process_token(Token::TagToken(start), line);
            return self.0.process_token(Token::TagToken(end), line);
        }
        self.0.process_token(token, line)
    }

    fn end(&self) {
        self.0.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.0
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

const MAX_HTML_DEPTH: usize = 128;
const MAX_HTML_TAGS: usize = 8192;

#[derive(Default)]
struct HtmlScan {
    open: Vec<html5ever::LocalName>,
    tags: usize,
    too_complex: bool,
    references: Vec<String>,
}

struct HtmlScanner {
    scan: RefCell<HtmlScan>,
    raw_text: bool,
}

impl TokenSink for HtmlScanner {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let Token::TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        let mut scan = self.scan.borrow_mut();
        if tag.kind == TagKind::EndTag {
            // Only matched top tags reduce this conservative lexical depth.
            // Implicit HTML closures may overestimate it, never authorize omissions.
            if scan.open.last() == Some(&tag.name) {
                scan.open.pop();
            }
            return TokenSinkResult::Continue;
        }
        scan.tags += 1;
        if self.raw_text && !matches!(tag.name.as_ref(), "script" | "style" | "link" | "meta") {
            for attr in &tag.attrs {
                if attr.name.local.as_ref() == "href" && attr.value.starts_with("entry://") {
                    scan.references.push(attr.value.to_string());
                }
            }
        }
        if !scan.too_complex {
            if !matches!(
                tag.name.as_ref(),
                "area"
                    | "base"
                    | "br"
                    | "col"
                    | "embed"
                    | "hr"
                    | "img"
                    | "input"
                    | "link"
                    | "meta"
                    | "param"
                    | "source"
                    | "track"
                    | "wbr"
            ) && !(tag.name.as_ref() == "a" && tag.self_closing)
            {
                scan.open.push(tag.name.clone());
            }
            scan.too_complex = scan.tags > MAX_HTML_TAGS || scan.open.len() > MAX_HTML_DEPTH;
        }
        // The complexity pass deliberately counts markup-like tokens even in
        // raw text: foreign-content integration points can change HTML parsing.
        if !self.raw_text {
            return TokenSinkResult::Continue;
        }
        use html5ever::tokenizer::states::{Rawtext, Rcdata, ScriptData};
        match tag.name.as_ref() {
            "script" => TokenSinkResult::RawData(ScriptData),
            "style" | "xmp" | "iframe" | "noembed" | "noframes" => {
                TokenSinkResult::RawData(Rawtext)
            }
            "title" | "textarea" => TokenSinkResult::RawData(Rcdata),
            "plaintext" => TokenSinkResult::Plaintext,
            _ => TokenSinkResult::Continue,
        }
    }
}

fn scan_html(html: &str, raw_text: bool) -> HtmlScan {
    let tokenizer = Tokenizer::new(
        HtmlScanner {
            scan: RefCell::new(HtmlScan::default()),
            raw_text,
        },
        Default::default(),
    );
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.scan.into_inner()
}

fn dom(html: &str) -> Option<RcDom> {
    if scan_html(html, false).too_complex {
        return None;
    }
    let tokenizer = Tokenizer::new(
        DictionaryTree(TreeBuilder::new(RcDom::default(), Default::default())),
        Default::default(),
    );
    let input = BufferQueue::default();
    input.push_back(html.into());
    while let TokenizerResult::Script(_) = tokenizer.feed(&input) {}
    tokenizer.end();
    let dom = tokenizer.sink.0.sink;
    // HTML recovery can change nesting. Check actual depth iteratively before
    // the small recursive dictionary walkers or serializer see the tree.
    let mut pending = vec![(dom.document.clone(), 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_HTML_DEPTH {
            return None;
        }
        pending.extend(
            node.children
                .borrow()
                .iter()
                .map(|node| (node.clone(), depth + 1)),
        );
    }
    Some(dom)
}

fn attr(node: &Handle, key: &str) -> Option<String> {
    if let NodeData::Element { attrs, .. } = &node.data {
        attrs
            .borrow()
            .iter()
            .find(|attr| attr.name.local.as_ref() == key)
            .map(|attr| attr.value.to_string())
    } else {
        None
    }
}

fn reference_target(node: &Handle) -> Option<String> {
    attr(node, "href").filter(|target| target.starts_with("entry://"))
}

fn has_class(node: &Handle, class: &str) -> bool {
    attr(node, "class")
        .is_some_and(|value| value.split_ascii_whitespace().any(|part| part == class))
}

fn tag(node: &Handle) -> &str {
    match &node.data {
        NodeData::Element { name, .. } => name.local.as_ref(),
        _ => "",
    }
}

fn invisible(node: &Handle) -> bool {
    matches!(tag(node), "script" | "style" | "link" | "meta")
}

fn inline(node: &Handle, out: &mut String, omit_number: bool) {
    if invisible(node)
        || (has_class(node, "entryDot")
            && reference_target(node).is_none()
            && node
                .children
                .borrow()
                .iter()
                .all(|child| match &child.data {
                    NodeData::Text { contents } => contents
                        .borrow()
                        .chars()
                        .all(|ch| ch.is_whitespace() || super::is_bullet(ch)),
                    _ => false,
                }))
        || (omit_number && has_class(node, "entryNum"))
    {
        return;
    }
    if let NodeData::Text { contents } = &node.data {
        out.push_str(&contents.borrow());
    }
    if has_class(node, "superscript") || tag(node) == "br" || tag(node) == "hr" {
        out.push(' ');
    }
    for child in node.children.borrow().iter() {
        inline(child, out, omit_number);
    }
    if let Some(target) = reference_target(node) {
        out.push_str(" (");
        out.push_str(&target);
        out.push(')');
    }
}

fn text(node: &Handle, omit_number: bool) -> String {
    let mut out = String::new();
    inline(node, &mut out, omit_number);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn other_text(node: &Handle) -> String {
    let mut html = Vec::new();
    html5ever::serialize(
        &mut html,
        &SerializableHandle::from(node.clone()),
        html5ever::serialize::SerializeOpts {
            traversal_scope: html5ever::serialize::TraversalScope::IncludeNode,
            ..Default::default()
        },
    )
    .expect("writing HTML to a Vec cannot fail");
    let html = String::from_utf8(html).expect("HTML serialization is UTF-8");
    let mut readable = super::html_to_text(&html);
    for target in reference_targets(&html) {
        readable.push_str("\nReference: ");
        readable.push_str(&target);
    }
    readable
}

fn descendants_with_class(node: &Handle, class: &str, out: &mut Vec<Handle>) {
    if has_class(node, class) {
        out.push(node.clone());
    }
    for child in node.children.borrow().iter() {
        descendants_with_class(child, class, out);
    }
}

pub(super) fn reference_targets(html: &str) -> Vec<String> {
    let Some(dom) = dom(html) else {
        return scan_html(html, true).references;
    };
    let mut targets = Vec::new();
    let mut pending = vec![dom.document.clone()];
    while let Some(node) = pending.pop() {
        if invisible(&node) {
            continue;
        }
        if let Some(target) = reference_target(&node) {
            targets.push(target);
        }
        pending.extend(node.children.borrow().iter().rev().cloned());
    }
    targets
}

pub(super) fn parse(html: &str, headword: &str) -> Option<Vec<Block>> {
    let dom = dom(html)?;
    let mut roots = Vec::new();
    descendants_with_class(&dom.document, "CK", &mut roots);
    let [root] = roots.as_slice() else {
        return None;
    };
    if reference_target(root).is_some() {
        return None;
    }
    let children = root.children.borrow();
    if !children.iter().any(|node| has_class(node, "DC"))
        || !children.iter().any(|node| has_class(node, "JS"))
    {
        return None;
    }
    // A partial fingerprint must never authorize omissions in the rest of a record.
    fn outside(node: &Handle, root: &Handle) -> bool {
        if std::rc::Rc::ptr_eq(node, root) || invisible(node) {
            return false;
        }
        if let NodeData::Text { contents } = &node.data
            && !contents.borrow().trim().is_empty()
        {
            return true;
        }
        if reference_target(node).is_some() {
            return true;
        }
        node.children
            .borrow()
            .iter()
            .any(|child| outside(child, root))
    }
    if outside(&dom.document, root) {
        return None;
    }
    let mut blocks = Vec::new();
    for child in children.iter() {
        if has_class(child, "DC") {
            let title = text(child, false);
            if title != headword {
                blocks.push(Block::Heading(title));
            }
        } else if has_class(child, "JS") {
            parse_group(child, headword, &mut blocks)?;
        } else if !text(child, false).is_empty() {
            blocks.push(Block::Other(other_text(child)));
        }
    }
    Some(blocks)
}

fn parse_group(node: &Handle, headword: &str, blocks: &mut Vec<Block>) -> Option<()> {
    if reference_target(node).is_some() {
        return None;
    }
    if !node
        .children
        .borrow()
        .iter()
        .any(|child| has_class(child, "CY"))
    {
        return None;
    }
    for child in node.children.borrow().iter() {
        if ["CY", "CZ", "JC", "YY"]
            .iter()
            .any(|class| has_class(child, class))
        {
            if reference_target(child).is_some() {
                return None;
            }
            let mut section = Vec::new();
            let mut containers = 0;
            for part in child.children.borrow().iter() {
                if has_class(part, "section_title") {
                    section.push(Block::Heading(text(part, false)));
                } else if has_class(part, "CX") {
                    parse_senses(part, headword, &mut section)?;
                    containers += 1;
                } else if !text(part, false).is_empty() {
                    if has_class(child, "YY") {
                        return None;
                    }
                    section.push(Block::Other(other_text(part)));
                }
            }
            if containers == 0 {
                return None;
            }
            if has_class(child, "YY") {
                // Only a titled, structurally recognized etymology is suppressible.
                if !section
                    .iter()
                    .any(|block| matches!(block, Block::Heading(_)))
                    || section.iter().any(|block| matches!(block, Block::Other(_)))
                {
                    return None;
                }
                blocks.push(Block::Etymology(section));
            } else {
                blocks.extend(section);
            }
        } else if !text(child, false).is_empty() {
            // Usage, derivations and unfamiliar sections are always retained.
            blocks.push(Block::Other(other_text(child)));
        }
    }
    Some(())
}

fn parse_senses(node: &Handle, headword: &str, blocks: &mut Vec<Block>) -> Option<()> {
    if reference_target(node).is_some() {
        return None;
    }
    let mut main = false;
    let mut subsense = false;
    let mut examples = 0;
    for child in node.children.borrow().iter() {
        // Nested semantic blocks indicate damage beyond ordinary inline recovery.
        for class in ["CX", "DX", "YX", "YD", "JX", "GZ", "LJ"] {
            let mut nested = Vec::new();
            for part in child.children.borrow().iter() {
                descendants_with_class(part, class, &mut nested);
            }
            if !nested.is_empty() {
                return None;
            }
        }
        let value = text(child, has_class(child, "JX"));
        if has_class(child, "YX") || has_class(child, "YD") {
            if value != headword && !value.is_empty() {
                blocks.push(Block::Heading(value));
            }
            main = false;
            subsense = false;
        } else if has_class(child, "DX") {
            blocks.push(Block::Label(value));
            main = false;
            subsense = false;
        } else if has_class(child, "JX") {
            let mut numbers = Vec::new();
            descendants_with_class(child, "entryNum", &mut numbers);
            if numbers.len() > 1 {
                return None;
            }
            let number = numbers
                .first()
                .map(|node| text(node, false))
                .unwrap_or_default();
            blocks.push(Block::Sense {
                number,
                text: value,
            });
            main = true;
            subsense = false;
            examples = 0;
        } else if has_class(child, "GZ") {
            let starts_subsense = value.starts_with('■');
            let value = if let Some(value) = value.strip_prefix('■') {
                subsense = true;
                value.trim().to_owned()
            } else {
                value
            };
            blocks.push(Block::Definition {
                text: value,
                subsense,
                starts_subsense,
            });
        } else if has_class(child, "LJ") {
            if !main || reference_target(child).is_some() {
                return None;
            }
            let mut lines = Vec::new();
            for part in child.children.borrow().iter() {
                let line = text(part, false);
                if !line.is_empty() {
                    if !has_class(part, "LY") && !has_class(part, "LS") {
                        return None;
                    }
                    lines.push(line);
                }
            }
            if !lines.is_empty() {
                blocks.push(Block::Example {
                    lines,
                    first_direct: main && !subsense && examples == 0,
                    subsense,
                });
                examples += 1;
            }
        } else if !value.is_empty() {
            blocks.push(Block::Other(other_text(child)));
            // Unknown intervening content cannot establish direct attachment.
            main = false;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_HTML_TAGS, dom, scan_html};

    #[test]
    fn complexity_preflight_bounds_nesting_including_foreign_raw_text() {
        for tag in ["div", "span", "span/"] {
            let nested = format!(
                "{}Meaning.{}",
                format!("<{tag}>").repeat(20_000),
                "</span>".repeat(20_000)
            );
            for html in [
                nested.clone(),
                format!("<svg><title>{nested}</title></svg>"),
            ] {
                assert!(scan_html(&html, false).too_complex);
                assert!(dom(&html).is_none());
            }
        }
        let wide = "<span></span>".repeat(MAX_HTML_TAGS + 1);
        assert!(scan_html(&wide, false).too_complex);
        assert!(dom(&wide).is_none());
    }

    #[test]
    fn complexity_budget_does_not_limit_ordinary_definition_text_length() {
        let html = format!(
            "<span class='CK'><span class='DC'>word</span><span class='JS'><span class='CY'><span class='CX'><span class='JX'>{}</span></span></span></span></span>",
            "Ordinary meaning. ".repeat(4000)
        );
        assert!(html.len() > 50_000);
        assert!(!scan_html(&html, false).too_complex);
        assert!(super::parse(&html, "word").is_some());
    }
}
