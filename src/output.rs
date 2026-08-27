use std::borrow::Cow;
use std::io::{self, Write};

pub fn write_record(
    writer: &mut impl Write,
    dictionary_name: &str,
    headword: &str,
    definition: &str,
    show_dictionary: bool,
    raw: bool,
    first: bool,
) -> io::Result<()> {
    if !first {
        writeln!(writer)?;
    }
    if show_dictionary {
        writeln!(writer, "[{dictionary_name}] {headword}")?;
    } else {
        writeln!(writer, "{headword}")?;
    }
    let definition = render_definition(definition, raw);
    writer.write_all(definition.as_bytes())?;
    if !definition.ends_with('\n') {
        writeln!(writer)?;
    }
    Ok(())
}

pub fn write_miss(writer: &mut impl Write, term: &str) -> io::Result<()> {
    writeln!(writer, "No entry found for: {term}")
}

/// Default query output is terminal-readable. `--raw` keeps the stored definition
/// unchanged because callers may pipe HTML or other markup onward.
fn render_definition(definition: &str, raw: bool) -> Cow<'_, str> {
    if raw || !contains_html_tag(definition) {
        return Cow::Borrowed(definition);
    }
    let readable = html_to_text(definition);
    if readable.trim().is_empty() {
        Cow::Borrowed(definition)
    } else {
        Cow::Owned(readable)
    }
}

fn contains_html_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'/' {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut text_run = String::new();
    let mut pos = 0;
    while pos < html.len() {
        let rest = &html[pos..];
        if rest.starts_with('<')
            && let Some(markup) = consume_markup(html, pos)
        {
            flush_text(&mut out, &mut text_run);
            match markup {
                Markup::Skip(next) => {
                    pos = next;
                    continue;
                }
                Markup::Break { next, blank } => {
                    push_break(&mut out, blank);
                    pos = next;
                    continue;
                }
            }
        }
        if rest.starts_with('&')
            && let Some((text, next)) = consume_entity(html, pos)
        {
            text_run.push_str(&text);
            pos = next;
            continue;
        }
        let ch = rest.chars().next().expect("remaining input is non-empty");
        text_run.push(ch);
        pos += ch.len_utf8();
    }
    flush_text(&mut out, &mut text_run);
    polish_readable(out)
}

enum Markup {
    Skip(usize),
    Break { next: usize, blank: bool },
}

struct Tag<'a> {
    name: &'a str,
    closing: bool,
    self_closing: bool,
    end: usize,
}

fn consume_markup(html: &str, pos: usize) -> Option<Markup> {
    let rest = &html[pos + 1..];
    if rest.starts_with("!--") {
        let end = match rest.find("-->") {
            Some(offset) => pos + 1 + offset + 3,
            None => html.len(),
        };
        return Some(Markup::Skip(end));
    }
    if rest.starts_with('!') {
        let end = match rest.find('>') {
            Some(offset) => pos + 1 + offset + 1,
            None => html.len(),
        };
        return Some(Markup::Skip(end));
    }

    let tag = parse_tag(html, pos)?;
    if !tag.closing && !tag.self_closing && tag_eq_any(tag.name, &["script", "style"]) {
        return Some(Markup::Skip(skip_element(html, tag.end, tag.name)));
    }
    if tag_eq_any(
        tag.name,
        &[
            "link", "meta", "base", "img", "input", "source", "track", "wbr", "area", "col",
        ],
    ) {
        return Some(Markup::Skip(tag.end));
    }
    if let Some(blank) = break_style(tag.name) {
        return Some(Markup::Break {
            next: tag.end,
            blank,
        });
    }
    Some(Markup::Skip(tag.end))
}

fn parse_tag(html: &str, pos: usize) -> Option<Tag<'_>> {
    let bytes = html.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'<' {
        return None;
    }
    let mut i = pos + 1;
    let closing = i < bytes.len() && bytes[i] == b'/';
    if closing {
        i += 1;
    }
    let name_start = i;
    if i >= bytes.len() || !bytes[i].is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    let name_end = i;
    let mut quote = None;
    let mut self_closing = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => {
                quote = Some(b);
                i += 1;
            }
            b'>' => {
                return Some(Tag {
                    name: &html[name_start..name_end],
                    closing,
                    self_closing: self_closing || html[name_end..i].trim_end().ends_with('/'),
                    end: i + 1,
                });
            }
            b'/' => {
                self_closing = true;
                i += 1;
            }
            _ => {
                if !b.is_ascii_whitespace() {
                    self_closing = false;
                }
                i += 1;
            }
        }
    }
    None
}

fn skip_element(html: &str, from: usize, name: &str) -> usize {
    let mut i = from;
    while i < html.len() {
        if html.as_bytes()[i] == b'<'
            && html[i + 1..].len() > name.len()
            && html.as_bytes()[i + 1] == b'/'
            && html[i + 2..]
                .get(..name.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        {
            let after_name = i + 2 + name.len();
            if let Some(rel) = html[after_name..].find('>') {
                return after_name + rel + 1;
            }
            return html.len();
        }
        i += html[i..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    html.len()
}

fn tag_eq_any(name: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| name.eq_ignore_ascii_case(option))
}

fn break_style(name: &str) -> Option<bool> {
    if name.eq_ignore_ascii_case("br") {
        return Some(false);
    }
    if name.eq_ignore_ascii_case("hr") {
        return Some(true);
    }
    if tag_eq_any(
        name,
        &[
            "p",
            "div",
            "tr",
            "li",
            "ul",
            "ol",
            "table",
            "thead",
            "tbody",
            "tfoot",
            "blockquote",
            "section",
            "article",
            "header",
            "footer",
            "dt",
            "dd",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "pre",
            "figure",
            "figcaption",
        ],
    ) {
        Some(false)
    } else {
        None
    }
}

fn consume_entity(html: &str, pos: usize) -> Option<(String, usize)> {
    let rest = &html[pos + 1..];
    let end = rest.find(';')?;
    if end == 0 || end > 12 {
        return None;
    }
    let body = &rest[..end];
    let next = pos + 1 + end + 1;
    let decoded = decode_entity_body(body)?;
    Some((decoded, next))
}

fn decode_entity_body(body: &str) -> Option<String> {
    if let Some(digits) = body.strip_prefix('#') {
        let code = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            digits.parse().ok()?
        };
        if code == 0 {
            return None;
        }
        return char::from_u32(code).map(String::from);
    }
    let replacement = match body {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "ndash" => "–",
        "mdash" => "—",
        "hellip" => "…",
        "lsquo" | "sbquo" => "‘",
        "rsquo" => "’",
        "ldquo" | "bdquo" => "“",
        "rdquo" => "”",
        "bull" => "•",
        "middot" => "·",
        "deg" => "°",
        "times" => "×",
        "divide" => "÷",
        "plusmn" => "±",
        _ => return None,
    };
    Some(replacement.to_string())
}

fn push_break(out: &mut String, blank: bool) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if blank && !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn flush_text(out: &mut String, text_run: &mut String) {
    if text_run.is_empty() {
        return;
    }
    push_text(out, text_run);
    text_run.clear();
}

fn push_text(out: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if needs_space(out, text) {
        out.push(' ');
    }
    out.push_str(text);
}

fn needs_space(out: &str, text: &str) -> bool {
    let Some(prev) = out.chars().next_back() else {
        return false;
    };
    let Some(next) = text.chars().next() else {
        return false;
    };
    if prev.is_whitespace() || next.is_whitespace() {
        return false;
    }
    if is_opening_punct(prev) || is_closing_punct(next) {
        return false;
    }
    true
}

fn is_opening_punct(ch: char) -> bool {
    matches!(
        ch,
        '(' | '[' | '{' | '"' | '\'' | '“' | '‘' | '「' | '『' | '《' | '（'
    )
}

fn is_closing_punct(ch: char) -> bool {
    matches!(
        ch,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
            | '%'
            | '"'
            | '\''
            | '”'
            | '’'
            | '」'
            | '』'
            | '》'
            | '，'
            | '。'
            | '；'
            | '：'
            | '、'
            | '）'
    )
}

fn polish_readable(text: String) -> String {
    let with_bullets = break_before_bullets(&text);
    let joined = attach_sense_numbers(&with_bullets);
    tidy_lines(&joined)
}

fn is_bullet(ch: char) -> bool {
    matches!(ch, '■' | '●' | '◆' | '▶' | '►' | '▪' | '▫')
}

fn break_before_bullets(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if is_bullet(ch) && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push(ch);
    }
    out
}

fn attach_sense_numbers(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::with_capacity(text.len() + 8);
    let mut i = 0;
    while i < lines.len() {
        if i + 1 < lines.len()
            && lines[i + 1]
                .trim_start()
                .chars()
                .next()
                .is_some_and(is_bullet)
            && let Some((prefix, number)) = split_trailing_sense_number(lines[i])
        {
            if !prefix.is_empty() {
                push_line(&mut out, prefix);
            }
            let combined = format!("{number} {}", lines[i + 1].trim_start());
            push_line(&mut out, &combined);
            i += 2;
            continue;
        }
        push_line(&mut out, lines[i]);
        i += 1;
    }
    out
}

fn split_trailing_sense_number(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_end();
    let bytes = trimmed.as_bytes();
    if !bytes.ends_with(b".") {
        return None;
    }
    let period = bytes.len() - 1;
    let mut digits = period;
    while digits > 0 && bytes[digits - 1].is_ascii_digit() {
        digits -= 1;
    }
    if digits == period {
        return None;
    }
    Some((trimmed[..digits].trim_end(), &trimmed[digits..]))
}

fn push_line(out: &mut String, line: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(line);
}

fn tidy_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut started = false;
    let mut pending_blank = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            pending_blank = started;
            continue;
        }
        if started {
            out.push('\n');
            if pending_blank {
                out.push('\n');
            }
        }
        out.push_str(line);
        started = true;
        pending_blank = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{write_miss, write_record};

    #[test]
    fn pads_definition_newline_and_separates_records() {
        let mut buf = Vec::new();
        write_record(&mut buf, "oxford", "a", "one", false, false, true).unwrap();
        write_record(&mut buf, "oxford", "b", "two\n", false, false, false).unwrap();
        assert_eq!(buf, b"a\none\n\nb\ntwo\n");
    }

    #[test]
    fn show_dictionary_uses_bracket_title_and_verbatim_definition() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "Oxford",
            "hello",
            "  keep \nand a blank\n",
            true,
            false,
            true,
        )
        .unwrap();
        assert_eq!(buf, b"[Oxford] hello\n  keep \nand a blank\n");
    }

    #[test]
    fn default_output_makes_html_definitions_readable() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "oxford",
            "hello",
            concat!(
                r#"<link rel="stylesheet" type="text/css" href="sf_cb.css"/>"#,
                r#"<span class="DC">hello</span>"#,
                r#"<span class="DX">(亦作 hallo 或 hullo)</span>"#,
                r#"<span class="DX">exclamation</span>"#,
                r#"<span class="entryNum">1.</span>"#,
                r#"<span class="entryDot">■</span>"#,
                "used as a greeting",
                r#"<span class="GZ">（用于打招呼或问候）哈罗，喂</span>"#,
                "<hr>",
                r#"<span class="section_title">语源</span>"#,
                r#"<span class="italic">hollo</span>"#,
            ),
            false,
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            buf,
            concat!(
                "hello\n",
                "hello (亦作 hallo 或 hullo) exclamation\n",
                "1. ■ used as a greeting （用于打招呼或问候）哈罗，喂\n",
                "\n",
                "语源 hollo\n",
            )
            .as_bytes()
        );
    }

    #[test]
    fn raw_keeps_stored_definition_markup() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "oxford",
            "hello",
            "<span>hello &amp; world</span>",
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(buf, b"hello\n<span>hello &amp; world</span>\n");
    }

    #[test]
    fn readable_output_strips_scripts_and_decodes_entities() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "oxford",
            "a",
            "<script>alert(1)</script><p>A&nbsp;B &amp; C</p><p>second</p>",
            false,
            false,
            true,
        )
        .unwrap();
        assert_eq!(buf, b"a\nA B & C\nsecond\n");
    }

    #[test]
    fn plain_text_with_less_than_stays_verbatim() {
        let mut buf = Vec::new();
        write_record(&mut buf, "oxford", "n", "n. a < b\n", false, false, true).unwrap();
        assert_eq!(buf, b"n\nn. a < b\n");
    }

    #[test]
    fn miss_uses_the_contract_prefix() {
        let mut buf = Vec::new();
        write_miss(&mut buf, "helo").unwrap();
        assert_eq!(buf, b"No entry found for: helo\n");
    }
}
