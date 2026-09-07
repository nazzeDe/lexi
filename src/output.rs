use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};

mod structured;

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub show_dictionary: bool,
    pub raw: bool,
    pub full: bool,
    pub terminal: Terminal,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Terminal {
    pub width: Option<usize>,
    pub color: bool,
}

impl Terminal {
    pub fn detect() -> Self {
        if !io::stdout().is_terminal() {
            return Self::default();
        }
        Self {
            width: terminal_size::terminal_size_of(io::stdout())
                .map(|(terminal_size::Width(width), _)| usize::from(width)),
            color: std::env::var_os("NO_COLOR").is_none(),
        }
    }
}

pub fn write_record(
    writer: &mut impl Write,
    dictionary_name: &str,
    headword: &str,
    definition: &str,
    options: Options,
    first: bool,
) -> io::Result<()> {
    if !first {
        writeln!(writer)?;
    }
    let title = if options.show_dictionary {
        format!("[{dictionary_name}] {headword}")
    } else {
        headword.to_owned()
    };
    let terminal = if options.raw {
        Terminal::default()
    } else {
        options.terminal
    };
    write_line(writer, &title, "", "", Style::Heading, terminal)?;
    if !options.raw
        && contains_html_tag(definition)
        && let Some(blocks) = structured::parse(definition, headword)
    {
        return write_structured(writer, &blocks, options);
    }
    let is_html = contains_html_tag(definition);
    let definition = render_definition(definition, options.raw);
    if !options.raw && is_html && terminal.width.is_some() {
        for line in definition.lines() {
            if line.is_empty() {
                writeln!(writer)?;
            } else {
                write_line(writer, line, "", "", Style::Plain, terminal)?;
            }
        }
        return Ok(());
    }
    writer.write_all(definition.as_bytes())?;
    if !definition.ends_with('\n') {
        writeln!(writer)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Style {
    Plain,
    Heading,
    Muted,
}

fn write_line(
    writer: &mut impl Write,
    text: &str,
    initial: &str,
    continuation: &str,
    style: Style,
    terminal: Terminal,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let rendered = if let Some(width) = terminal.width {
        let width = width.max(1);
        // Leave room for a wide glyph even when the terminal is narrower than
        // the usual indentation. Styling is applied after wrapping, not measured.
        let room = width.saturating_sub(2);
        let initial = if initial.len() <= room { initial } else { "" };
        let continuation = if continuation.len() <= room {
            continuation
        } else {
            ""
        };
        textwrap::fill(
            text,
            textwrap::Options::new(width)
                .initial_indent(initial)
                .subsequent_indent(continuation),
        )
    } else {
        format!("{initial}{text}")
    };
    let code = match style {
        Style::Plain => "",
        Style::Heading => "\x1b[1m",
        Style::Muted => "\x1b[2m",
    };
    if terminal.color && !code.is_empty() {
        writeln!(writer, "{code}{rendered}\x1b[0m")
    } else {
        writeln!(writer, "{rendered}")
    }
}

#[derive(Default)]
struct Omissions {
    examples: usize,
    etymology: bool,
}

fn example_count(blocks: &[structured::Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            structured::Block::Example { .. } => 1,
            structured::Block::Etymology(blocks) => example_count(blocks),
            _ => 0,
        })
        .sum()
}

fn write_structured(
    writer: &mut impl Write,
    blocks: &[structured::Block],
    options: Options,
) -> io::Result<()> {
    let mut omissions = Omissions::default();
    write_blocks(writer, blocks, options, &mut omissions)?;
    let mut omitted = Vec::new();
    if omissions.examples > 0 {
        let noun = if omissions.examples == 1 {
            "example"
        } else {
            "examples"
        };
        omitted.push(format!("{} {noun}", omissions.examples));
    }
    if omissions.etymology {
        omitted.push("etymology".to_owned());
    }
    if !omitted.is_empty() {
        writeln!(writer)?;
        write_line(
            writer,
            &format!(
                "Omitted: {}; use --full to show all.",
                omitted.join(" and ")
            ),
            "",
            "",
            Style::Muted,
            options.terminal,
        )?;
    }
    Ok(())
}

fn is_part_of_speech(label: &str) -> bool {
    matches!(
        label,
        "noun"
            | "verb"
            | "adjective"
            | "adverb"
            | "exclamation"
            | "interjection"
            | "pronoun"
            | "preposition"
            | "conjunction"
            | "determiner"
            | "predeterminer"
            | "article"
            | "number"
            | "ordinal number"
            | "cardinal number"
            | "auxiliary verb"
            | "modal verb"
            | "prefix"
            | "suffix"
            | "combining form"
    )
}

fn write_blocks(
    writer: &mut impl Write,
    blocks: &[structured::Block],
    options: Options,
    omissions: &mut Omissions,
) -> io::Result<()> {
    use structured::Block;
    let mut definition_indent = 2;
    for block in blocks {
        match block {
            Block::Heading(text) => {
                writeln!(writer)?;
                write_line(writer, text, "", "", Style::Heading, options.terminal)?;
            }
            Block::Label(text) => {
                if !text.is_empty() {
                    writeln!(writer)?;
                    let style = if is_part_of_speech(text) {
                        Style::Heading
                    } else {
                        Style::Muted
                    };
                    write_line(writer, text, "  ", "  ", style, options.terminal)?;
                }
            }
            Block::Sense { number, text } => {
                if !number.is_empty() || !text.is_empty() {
                    writeln!(writer)?;
                }
                definition_indent = 2 + if number.is_empty() {
                    0
                } else {
                    textwrap::core::display_width(number) + 1
                };
                let continuation = " ".repeat(definition_indent);
                let text = if number.is_empty() {
                    text.clone()
                } else {
                    format!("{number} {text}")
                };
                write_line(
                    writer,
                    text.trim_end(),
                    "  ",
                    &continuation,
                    Style::Plain,
                    options.terminal,
                )?;
            }
            Block::Definition {
                text,
                subsense,
                starts_subsense,
            } => {
                let initial =
                    " ".repeat(definition_indent + usize::from(*subsense && !starts_subsense) * 2);
                let continuation = " ".repeat(definition_indent + usize::from(*subsense) * 2);
                let text = if *starts_subsense {
                    writeln!(writer)?;
                    format!("- {text}")
                } else {
                    text.clone()
                };
                write_line(
                    writer,
                    &text,
                    &initial,
                    &continuation,
                    Style::Plain,
                    options.terminal,
                )?;
            }
            Block::Example {
                lines,
                first_direct,
                subsense,
            } => {
                if options.full || *first_direct {
                    writeln!(writer)?;
                    let initial = " ".repeat(definition_indent + usize::from(*subsense) * 2);
                    let continuation = format!("{initial}  ");
                    for (index, line) in lines.iter().enumerate() {
                        let (text, indent) = if index == 0 {
                            (format!("> {line}"), &initial)
                        } else {
                            (line.clone(), &continuation)
                        };
                        write_line(
                            writer,
                            &text,
                            indent,
                            &continuation,
                            Style::Muted,
                            options.terminal,
                        )?;
                    }
                } else {
                    omissions.examples += 1;
                }
            }
            Block::Etymology(blocks) => {
                if options.full {
                    write_blocks(writer, blocks, options, omissions)?;
                } else if !blocks.is_empty() {
                    omissions.etymology = true;
                    omissions.examples += example_count(blocks);
                }
            }
            Block::Other(text) => {
                for line in text.lines() {
                    write_line(writer, line, "  ", "  ", Style::Plain, options.terminal)?;
                }
            }
        }
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
    let mut readable = html_to_text(definition);
    for target in structured::reference_targets(definition) {
        readable.push_str("\nReference: ");
        readable.push_str(&target);
    }
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
    use super::{Options, write_miss, write_record};

    #[test]
    fn pads_definition_newline_and_separates_records() {
        let mut buf = Vec::new();
        write_record(&mut buf, "oxford", "a", "one", Options::default(), true).unwrap();
        write_record(&mut buf, "oxford", "b", "two\n", Options::default(), false).unwrap();
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
            Options {
                show_dictionary: true,
                ..Options::default()
            },
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
            Options::default(),
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
            Options {
                raw: true,
                ..Options::default()
            },
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
            Options::default(),
            true,
        )
        .unwrap();
        assert_eq!(buf, b"a\nA B & C\nsecond\n");
    }

    #[test]
    fn plain_text_with_less_than_stays_verbatim() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "oxford",
            "n",
            "n. a < b\n",
            Options::default(),
            true,
        )
        .unwrap();
        assert_eq!(buf, b"n\nn. a < b\n");
    }

    fn rendered(headword: &str, definition: &str, options: Options) -> String {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "arbitrary import name",
            headword,
            definition,
            options,
            true,
        )
        .unwrap();
        String::from_utf8(buf).unwrap()
    }

    const POST: &str = include_str!("../tests/fixtures/post.html");
    const HELLO: &str = include_str!("../tests/fixtures/hello.html");
    const RUN: &str = include_str!("../tests/fixtures/run.html");

    #[test]
    fn structured_post_preserves_groups_senses_and_counts_actual_omissions() {
        let output = rendered("post", POST, Options::default());
        assert!(
            output.starts_with("post\n\npost 1\n\n  noun\n\n  1. A marker.\n     标记。\n"),
            "{output}"
        );
        assert_eq!(output.matches("\npost\n").count(), 0);
        assert!(output.contains("\n     > First marker example.\n       第一个例句。"));
        assert!(!output.contains("Second marker example."));
        assert!(!output.contains("Subsense example."));
        assert!(
            output.contains("\n     - (rare) A distinct subsense.\n       （罕见）不同的子义项。")
        );
        assert!(output.contains("Display example."));
        assert!(!output.contains("An old source."));
        assert!(output.ends_with("Omitted: 2 examples and etymology; use --full to show all.\n"));
        assert_eq!(output.matches("Omitted:").count(), 1);
        let positions = ["post 1", "To display.", "post 2", "By delivery.", "post 3"]
            .map(|part| output.find(part).unwrap());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn full_restores_examples_etymology_and_leaves_raw_verbatim() {
        let full = rendered(
            "post",
            POST,
            Options {
                full: true,
                ..Options::default()
            },
        );
        for content in [
            "First marker example.",
            "Second marker example.",
            "Subsense example.",
            "Display example.",
            "An old source.",
        ] {
            assert!(full.contains(content), "{content}");
        }
        assert!(!full.contains("Omitted:"));
        let raw = rendered(
            "post",
            POST,
            Options {
                raw: true,
                terminal: super::Terminal {
                    color: true,
                    width: Some(12),
                },
                ..Options::default()
            },
        );
        assert_eq!(raw, format!("post\n{POST}"));
    }

    #[test]
    fn footer_reports_only_content_actually_hidden_and_does_not_promote_subsense_examples() {
        let wrap = |body: &str, section: &str| {
            format!(
                "<span class='CK'><span class='DC'>word</span><span class='JS'><span class='CY'><span class='CX'>{body}</span></span>{section}</span></span>"
            )
        };
        let body = "<span class='JX'>A main meaning.</span><span class='GZ'>■A subsense.</span><span class='LJ'><span class='LY'>Hidden example.</span></span><span class='JX'>Second meaning.</span><span class='LJ'><span class='LS'>Visible translation only.</span></span>";
        let output = rendered("word", &wrap(body, ""), Options::default());
        assert!(output.contains("A subsense."));
        assert!(!output.contains("Hidden example."));
        assert!(output.contains("Visible translation only."));
        assert!(output.ends_with("Omitted: 1 example; use --full to show all.\n"));
        let etymology = "<span class='YY'><hr><span class='section_title'>Origin</span><span class='CX'><span class='JX'>Earlier form.</span><span class='LJ'><span class='LY'>Origin example.</span></span></span></span>";
        let output = rendered(
            "word",
            &wrap("<span class='JX'>Meaning.</span>", etymology),
            Options::default(),
        );
        assert!(output.ends_with("Omitted: 1 example and etymology; use --full to show all.\n"));
        let etymology = etymology.replace(
            "<span class='LJ'><span class='LY'>Origin example.</span></span>",
            "",
        );
        let output = rendered(
            "word",
            &wrap("<span class='JX'>Meaning.</span>", &etymology),
            Options::default(),
        );
        assert!(output.ends_with("Omitted: etymology; use --full to show all.\n"));
        let ambiguous = body.replace(
            "<span class='LY'>Hidden example.</span>",
            "<span class='future'>Unrecognized example content.</span>",
        );
        let html = wrap(&ambiguous, "");
        assert_eq!(
            rendered("word", &html, Options::default()),
            format!("word\n{}\n", super::html_to_text(&html))
        );
    }

    #[test]
    fn malformed_inline_tags_recover_in_order_and_non_decorative_marker_text_survives() {
        let html = "<span class='CK'><span class='DC'>word</span><span class='JS'><span class='CY'><span class='CX'><span class='JX'><span class='entryNum'>1.</span><span class='entryDot'>important</span>A <b>bold <i>and</b> italic</i> meaning.</span><span class='GZ'>Translation.</span></span></span></span></span>";
        let output = rendered("word", html, Options::default());
        assert_eq!(
            output,
            "word\n\n  1. importantA bold and italic meaning.\n     Translation.\n"
        );
        assert!(!output.contains("Omitted:"));
    }

    #[test]
    fn missing_labels_or_translations_are_not_invented() {
        let output = rendered("hello", HELLO, Options::default());
        assert!(output.contains("Hello, friend!"));
        assert!(!output.contains("noun"));
        assert!(!output.contains("verb"));
        assert!(output.contains("  (pl. -os)\n\n  1. A spoken greeting.\n\n  (-oes, -oed)"));
        assert!(!output.contains("Omitted:"));
    }

    #[test]
    fn phrases_follow_the_same_example_rule_and_keep_links_unknown_sections() {
        let output = rendered("run", RUN, Options::default());
        assert!(output.contains("First phrase example."));
        assert!(!output.contains("Second phrase example."));
        assert!(!output.contains("Phrase subsense example."));
        for content in [
            "Another meaning.",
            "A phrase subsense.",
            "run away",
            "run over",
            "见 over (entry://over)",
            "run with",
            "只有译文的例句。",
            "runner",
            "A usage note.",
            "Unfamiliar section",
            "Keep every word.",
            "entry://future",
        ] {
            assert!(output.contains(content), "missing {content}: {output}");
        }
        assert!(output.ends_with("Omitted: 2 examples and etymology; use --full to show all.\n"));
        assert!(!output.contains("  verb\n"));
    }

    #[test]
    fn unknown_and_unreliable_html_fall_back_whole_without_omissions() {
        for html in [
            "<div>Unknown format <span class=\"YY\">keep etymology</span><span class=\"LJ\">keep example</span></div>",
            "<span class=\"CK\"><span class=\"DC\">word</span><span class=\"JS\"><span class=\"CY\"><span class=\"CX\"><span class=\"JX\">damaged<span class=\"GZ\">nested</span></span></span></span><span class=\"YY\">keep etymology</span></span></span>",
            "outside<span class=\"CK\"><span class=\"DC\">word</span><span class=\"JS\"><span class=\"CY\"><span class=\"CX\"><span class=\"JX\">inside</span></span></span></span></span>",
        ] {
            let output = rendered("word", html, Options::default());
            assert_eq!(output, format!("word\n{}\n", super::html_to_text(html)));
            assert!(!output.contains("Omitted:"));
            assert_eq!(
                output,
                rendered(
                    "word",
                    html,
                    Options {
                        full: true,
                        ..Options::default()
                    }
                )
            );
        }
        let link = rendered(
            "alias",
            "<a href='entry://target'>Visible target</a>",
            Options::default(),
        );
        assert_eq!(link, "alias\nVisible target\nReference: entry://target\n");
    }

    #[test]
    fn empty_entry_anchors_survive_every_structural_boundary() {
        let anchor = "<a href='entry://target'></a>";
        let wrap = |body: &str| {
            format!(
                "<span class='CK'><span class='DC'>word</span><span class='JS'><span class='CY'><span class='CX'>{body}</span></span></span></span>"
            )
        };
        let html = wrap("<span class='JX'>Meaning.</span>");
        let mut cases = vec![format!("{html}{anchor}")];
        for class in ["CK", "DC", "JS", "CY", "CX", "JX"] {
            cases.push(html.replace(
                &format!("class='{class}'>"),
                &format!("class='{class}'>{anchor}"),
            ));
            cases.push(html.replace(
                &format!("class='{class}'>"),
                &format!("class='{class}' href='entry://target'>"),
            ));
        }
        for class in ["DX", "GZ", "YX", "YD", "entryDot", "entryNum"] {
            cases.push(wrap(&format!(
                "<span class='JX'>Meaning.<span class='{class}'>{anchor}</span></span>"
            )));
        }
        for class in ["DX", "GZ", "YX", "YD"] {
            cases.push(wrap(&format!(
                "<span class='JX'>Meaning.</span><span class='{class}'>{anchor}</span>"
            )));
        }
        cases.push(wrap(
            "<span class='JX'>Meaning.<a class='entryDot' href='entry://target'></a></span>",
        ));
        cases.push(wrap(
            "<span class='JX'>Meaning.</span><span class='LJ' href='entry://target'></span>",
        ));
        cases.push(wrap(&format!(
            "<span class='JX'>Meaning.</span><span class='LJ'>{anchor}</span>"
        )));
        cases.push(wrap(&format!("<span class='JX'>Meaning.</span><span class='LJ'><span class='LY'>{anchor}</span></span>")));
        cases.push(html.replace("</span></span></span></span>", &format!("</span></span><span class='YY'><span class='section_title'>Origin</span><span class='CX'><span class='JX'>Earlier.</span></span>{anchor}</span></span></span>")));
        for html in cases {
            for full in [false, true] {
                let output = rendered(
                    "word",
                    &html,
                    Options {
                        full,
                        ..Options::default()
                    },
                );
                assert_eq!(
                    output.matches("entry://target").count(),
                    1,
                    "{html}: {output}"
                );
                assert!(!output.contains("Omitted:"), "{output}");
            }
            assert_eq!(
                rendered(
                    "word",
                    &html,
                    Options {
                        raw: true,
                        ..Options::default()
                    }
                ),
                format!("word\n{html}\n")
            );
        }
    }

    #[test]
    fn over_complex_html_falls_back_without_losing_text_references_or_raw() {
        for tag in ["div", "span"] {
            let html = format!(
                "{}Meaning.<a href='entry://target&amp;more'></a>{}",
                format!("<{tag}>").repeat(20_000),
                format!("</{tag}>").repeat(20_000)
            );
            assert!(super::structured::parse(&html, "word").is_none());
            for full in [false, true] {
                let output = rendered(
                    "word",
                    &html,
                    Options {
                        full,
                        ..Options::default()
                    },
                );
                assert_eq!(output, "word\nMeaning.\nReference: entry://target&more\n");
            }
            assert_eq!(
                rendered(
                    "word",
                    &html,
                    Options {
                        raw: true,
                        ..Options::default()
                    }
                ),
                format!("word\n{html}\n")
            );
        }
        let html = format!(
            "{}<a href='entry://last'></a>",
            "<span>x</span>".repeat(8193)
        );
        assert!(super::structured::parse(&html, "word").is_none());
        assert_eq!(
            super::structured::reference_targets(&html),
            ["entry://last"]
        );
    }

    #[test]
    fn reference_scans_respect_raw_text_attributes_and_bounded_foreign_content() {
        let html = "<script>\"<a href='entry://script'>\"</script><style><a href='entry://style'></style><textarea><a href='entry://textarea'></textarea><!-- <a href='entry://comment'> --><a href='entry://real&amp;target'></a>";
        for html in [
            html.to_owned(),
            format!("{}{}{html}", "<div>".repeat(200), "</div>".repeat(200)),
        ] {
            assert_eq!(
                super::structured::reference_targets(&html),
                ["entry://real&target"]
            );
        }
        assert_eq!(
            super::structured::reference_targets(
                "<svg><title><a href='entry://foreign'>word</a></title></svg>"
            ),
            ["entry://foreign"]
        );
    }

    #[test]
    fn pos_headings_are_bold_but_inflections_stay_muted_without_inference() {
        let options = Options {
            terminal: super::Terminal {
                width: None,
                color: true,
            },
            ..Options::default()
        };
        let post = rendered("post", POST, options);
        for label in ["noun", "verb", "adverb"] {
            assert!(post.contains(&format!("\x1b[1m  {label}\x1b[0m")), "{post}");
        }
        let hello = rendered("hello", HELLO, options);
        assert!(hello.contains("\x1b[1m  exclamation\x1b[0m"));
        assert!(hello.contains("\x1b[2m  (pl. -os)\x1b[0m"));
        assert!(!hello.contains("  noun"));
        assert!(!hello.contains("  verb"));
    }

    #[test]
    fn terminal_wrapping_is_cjk_aware_and_styles_do_not_change_content() {
        let line = "English 中文释义 words 中文释义";
        let mut plain = Vec::new();
        let mut color = Vec::new();
        for (buf, enabled) in [(&mut plain, false), (&mut color, true)] {
            super::write_line(
                buf,
                line,
                "  ",
                "  ",
                super::Style::Heading,
                super::Terminal {
                    width: Some(16),
                    color: enabled,
                },
            )
            .unwrap();
        }
        let plain = String::from_utf8(plain).unwrap();
        let color = String::from_utf8(color).unwrap();
        assert_eq!(color.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
        assert!(plain.lines().count() > 1);
        for line in plain.lines() {
            assert!(line.starts_with("  "));
            assert!(textwrap::core::display_width(line) <= 16, "{line}");
        }
        for width in [0, 1, 2, 3, 4, 5] {
            let output = rendered(
                "post",
                POST,
                Options {
                    terminal: super::Terminal {
                        width: Some(width),
                        color: true,
                    },
                    ..Options::default()
                },
            );
            assert!(output.contains("\x1b[1m"));
        }
        let pipe = rendered("post", POST, Options::default());
        assert!(!pipe.contains('\x1b'));
        assert!(pipe.contains("Omitted: 2 examples and etymology; use --full to show all."));
        let plain_definition = "  pure text 中文 that must not wrap\n";
        let output = rendered(
            "text",
            plain_definition,
            Options {
                terminal: super::Terminal {
                    width: Some(4),
                    color: false,
                },
                ..Options::default()
            },
        );
        assert!(output.ends_with(plain_definition));
    }

    #[test]
    fn miss_uses_the_contract_prefix() {
        let mut buf = Vec::new();
        write_miss(&mut buf, "helo").unwrap();
        assert_eq!(buf, b"No entry found for: helo\n");
    }
}
