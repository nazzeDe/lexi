use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::record::Entry;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonlObject {
    headword: String,
    definition: String,
}

pub struct Reader {
    path: PathBuf,
    reader: BufReader<File>,
    line: String,
    line_number: usize,
}

pub fn open(path: impl AsRef<Path>) -> Result<Reader> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path)
        .with_context(|| format!("failed to open JSONL file {}", path.display()))?;
    let mut reader = BufReader::new(file);
    skip_utf8_bom(&mut reader)
        .with_context(|| format!("failed to read JSONL file {}", path.display()))?;
    Ok(Reader {
        path,
        reader,
        line: String::new(),
        line_number: 0,
    })
}

impl Iterator for Reader {
    type Item = Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line.clear();
            match self.reader.read_line(&mut self.line) {
                Ok(0) => return None,
                Ok(_) => {
                    self.line_number += 1;
                    trim_jsonl_terminator(&mut self.line);
                    if self.line.trim().is_empty() {
                        continue;
                    }
                    return Some(parse_record(&self.path, self.line_number, &self.line));
                }
                Err(error) => {
                    self.line_number += 1;
                    return Some(Err(anyhow::Error::new(error).context(format!(
                        "failed to read {} at line {}",
                        self.path.display(),
                        self.line_number
                    ))));
                }
            }
        }
    }
}

fn skip_utf8_bom(reader: &mut BufReader<File>) -> std::io::Result<()> {
    // A leading UTF-8 BOM is allowed and is not part of line 1.
    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
    let available = reader.fill_buf()?;
    if available.starts_with(&BOM) {
        reader.consume(BOM.len());
    }
    Ok(())
}

fn trim_jsonl_terminator(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

fn parse_record(path: &Path, line_number: usize, line: &str) -> Result<Entry> {
    let context = || {
        format!(
            "invalid JSONL record in {} at line {line_number}",
            path.display()
        )
    };
    let value: serde_json::Value = serde_json::from_str(line).with_context(context)?;
    if !value.is_object() {
        return Err(anyhow!("JSONL record must be a JSON object")).with_context(context);
    }
    let record: JsonlObject = serde_json::from_value(value).with_context(context)?;
    Entry::new(record.headword, record.definition).with_context(context)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::open;
    use crate::record::Entry;

    fn collect(contents: impl AsRef<[u8]>) -> anyhow::Result<Vec<Entry>> {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("sample.jsonl");
        fs::write(&path, contents).unwrap();
        open(&path)?.collect()
    }

    fn error_text(contents: impl AsRef<[u8]>) -> (String, PathBuf) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("sample.jsonl");
        fs::write(&path, contents).unwrap();
        let error = format!(
            "{:#}",
            open(&path)
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap_err()
        );
        (error, path)
    }

    #[test]
    fn reads_legal_records_and_preserves_definition_bytes() {
        let entries = collect(
            "{\"headword\":\"hello\",\"definition\":\"interjection\\n1. 你好\"}\n\
             {\"headword\":\"take off\",\"definition\":\"  keep spaces  \"}\n\
             {\"headword\":\"hello\",\"definition\":\"duplicate\"}\n",
        )
        .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].headword(), "hello");
        assert_eq!(entries[0].definition(), "interjection\n1. 你好");
        assert_eq!(entries[1].headword(), "take off");
        assert_eq!(entries[1].definition(), "  keep spaces  ");
        assert_eq!(entries[2].headword(), "hello");
        assert_eq!(entries[2].definition(), "duplicate");
    }

    #[test]
    fn accepts_bom_crlf_lf_and_blank_lines() {
        let mut contents = b"\xEF\xBB\xBF".to_vec();
        contents.extend_from_slice(
            b"{\"headword\":\"a\",\"definition\":\"one\"}\r\n \t \r\n\n{\"headword\":\"b\",\"definition\":\"two\"}\n",
        );
        let entries = collect(contents).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.headword(), entry.definition()))
                .collect::<Vec<_>>(),
            vec![("a", "one"), ("b", "two")]
        );
    }

    #[test]
    fn reports_every_strict_validation_class_with_file_line_numbers() {
        let cases: &[(&[u8], &str, &str)] = &[
            (b"{not json}\n", "at line 1", "key must be a string"),
            (b"[\"hello\"]\n", "at line 1", "JSONL record must be a JSON object"),
            (b"[\"hello\",\"world\"]\n", "at line 1", "JSONL record must be a JSON object"),
            (b"\"hello\"\n", "at line 1", "JSONL record must be a JSON object"),
            (b"{\"definition\":\"only\"}\n", "at line 1", "missing field `headword`"),
            (b"{\"headword\":\"only\"}\n", "at line 1", "missing field `definition`"),
            (
                b"{\"headword\":\"a\",\"definition\":\"b\",\"extra\":true}\n",
                "at line 1",
                "unknown field `extra`",
            ),
            (
                b"{\"headword\":1,\"definition\":\"b\"}\n",
                "at line 1",
                "invalid type",
            ),
            (
                b"{\"headword\":\"a\",\"definition\":null}\n",
                "at line 1",
                "invalid type",
            ),
            (
                b"{\"headword\":\"\",\"definition\":\"b\"}\n",
                "at line 1",
                "headword must be a non-empty string",
            ),
            (
                b"{\"headword\":\" a\",\"definition\":\"b\"}\n",
                "at line 1",
                "leading or trailing whitespace",
            ),
            (
                b"{\"headword\":\"a\\tb\",\"definition\":\"b\"}\n",
                "at line 1",
                "control characters",
            ),
            (
                b"{\"headword\":\"a\\nb\",\"definition\":\"b\"}\n",
                "at line 1",
                "control characters",
            ),
            (
                b"{\"headword\":\"a\",\"definition\":\"\"}\n",
                "at line 1",
                "definition must be a non-whitespace string",
            ),
            (
                b"{\"headword\":\"a\",\"definition\":\" \\t \"}\n",
                "at line 1",
                "definition must be a non-whitespace string",
            ),
            (
                b"{\"headword\":\"ok\",\"definition\":\"yes\"}\n\n{\"headword\":\"\",\"definition\":\"no\"}\n",
                "at line 3",
                "headword must be a non-empty string",
            ),
        ];

        for (contents, line, reason) in cases {
            let (error, path) = error_text(*contents);
            assert!(
                error.contains(&path.display().to_string()),
                "missing path in {error}"
            );
            assert!(error.contains(line), "missing {line} in {error}");
            assert!(error.contains(reason), "missing {reason} in {error}");
        }
    }

    #[test]
    fn reports_invalid_utf8_with_the_failing_line_number() {
        let mut contents = b"{\"headword\":\"ok\",\"definition\":\"yes\"}\n".to_vec();
        contents.extend_from_slice(b"\xff\n");
        let (error, path) = error_text(contents);
        assert!(error.contains(&path.display().to_string()));
        assert!(error.contains("at line 2"), "{error}");
    }

    #[test]
    fn missing_file_is_an_open_error() {
        let error = match open("/no/such/lexi-import.jsonl") {
            Ok(_) => panic!("missing file should fail"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("failed to open JSONL file"));
        assert!(error.contains("/no/such/lexi-import.jsonl"));
    }
}
