use anyhow::{Result, bail};

/// Unicode lowercase key used for dictionary uniqueness and headword candidates.
/// Rust's `to_lowercase` is required; SQLite `LOWER()` is not Unicode-aware enough.
fn folded_key(value: &str) -> String {
    value.to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryName {
    display: String,
    normalized: String,
}

impl DictionaryName {
    pub fn parse(raw: &str) -> Result<Self> {
        let display = raw.trim();
        if display.is_empty() {
            bail!("dictionary name must not be empty");
        }
        Ok(Self {
            normalized: folded_key(display),
            display: display.to_string(),
        })
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    headword: String,
    folded_headword: String,
    definition: String,
}

impl Entry {
    pub fn new(headword: String, definition: String) -> Result<Self> {
        if headword.is_empty() {
            bail!("headword must be a non-empty string");
        }
        if headword.chars().any(char::is_control) {
            bail!("headword must not contain control characters");
        }
        if headword.trim() != headword {
            bail!("headword must not contain leading or trailing whitespace");
        }
        if definition.trim().is_empty() {
            bail!("definition must be a non-whitespace string");
        }
        Ok(Self {
            folded_headword: folded_key(&headword),
            headword,
            definition,
        })
    }

    pub fn headword(&self) -> &str {
        &self.headword
    }

    pub fn folded_headword(&self) -> &str {
        &self.folded_headword
    }

    pub fn definition(&self) -> &str {
        &self.definition
    }
}

#[cfg(test)]
mod tests {
    use super::{DictionaryName, Entry};

    #[test]
    fn dictionary_name_trims_and_preserves_display_while_folding_case() {
        let name = DictionaryName::parse("  Oxford English  ").unwrap();
        assert_eq!(name.display(), "Oxford English");
        assert_eq!(name.normalized(), "oxford english");
        assert_eq!(
            DictionaryName::parse("Wörter").unwrap().normalized(),
            "wörter"
        );
        assert!(DictionaryName::parse("   \t  ").is_err());
    }

    #[test]
    fn entry_keeps_internal_spaces_and_definition_text() {
        let entry = Entry::new("take off".into(), "  keep spaces \nand a newline".into()).unwrap();
        assert_eq!(entry.headword(), "take off");
        assert_eq!(entry.folded_headword(), "take off");
        assert_eq!(entry.definition(), "  keep spaces \nand a newline");
        assert_eq!(
            Entry::new("Wörter".into(), "def".into())
                .unwrap()
                .folded_headword(),
            "wörter"
        );
    }

    #[test]
    fn entry_rejects_empty_control_and_surrounding_whitespace_headwords() {
        assert!(Entry::new(String::new(), "def".into()).is_err());
        assert!(Entry::new(" hello".into(), "def".into()).is_err());
        assert!(Entry::new("hello ".into(), "def".into()).is_err());
        assert!(Entry::new("hel\tlo".into(), "def".into()).is_err());
        assert!(Entry::new("hel\nlo".into(), "def".into()).is_err());
        assert!(Entry::new("a".into(), String::new()).is_err());
        assert!(Entry::new("a".into(), " \t ".into()).is_err());
    }
}
