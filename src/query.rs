use std::io::Write;

use anyhow::{Context, Result, bail};

use crate::output;
use crate::record::{DictionaryName, QueryTerm};
use crate::storage::{Storage, StoredEntry};

#[derive(Debug)]
pub enum Outcome {
    AllFound,
    SomeMissing,
}

pub fn run(
    storage: &Storage,
    terms: &[QueryTerm],
    dictionaries: &[DictionaryName],
    show_dictionary: bool,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Result<Outcome> {
    if !storage.has_any_dictionary()? {
        bail!("no dictionaries are installed");
    }
    let scope = storage.resolve_dictionaries(dictionaries)?;

    let mut missing = false;
    let mut first_record = true;
    for term in terms {
        let matches = select_matches(term, storage.lookup_folded(term.folded(), &scope)?);
        if matches.is_empty() {
            output::write_miss(stderr, term.text())
                .context("failed to write query diagnostic to stderr")?;
            missing = true;
            continue;
        }
        for entry in &matches {
            output::write_record(
                stdout,
                entry.dictionary_name(),
                entry.headword(),
                entry.definition(),
                show_dictionary,
                first_record,
            )
            .context("failed to write query result to stdout")?;
            first_record = false;
        }
    }

    Ok(if missing {
        Outcome::SomeMissing
    } else {
        Outcome::AllFound
    })
}

fn select_matches(term: &QueryTerm, candidates: Vec<StoredEntry>) -> Vec<StoredEntry> {
    // Exact original-headword matches win across the whole selected set;
    // dictionaries never fall back independently.
    if candidates
        .iter()
        .any(|entry| entry.headword() == term.text())
    {
        candidates
            .into_iter()
            .filter(|entry| entry.headword() == term.text())
            .collect()
    } else {
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, run, select_matches};
    use crate::record::{DictionaryName, Entry, QueryTerm};
    use crate::storage::Storage;
    use tempfile::TempDir;

    fn entry(headword: &str, definition: &str) -> Entry {
        Entry::new(headword.to_string(), definition.to_string()).unwrap()
    }

    fn term(raw: &str) -> QueryTerm {
        QueryTerm::parse(raw).unwrap()
    }

    fn storage_with(dictionaries: &[(&str, &[(&str, &str)])]) -> (TempDir, Storage) {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(directory.path().join("lexi.db")).unwrap();
        for (name, records) in dictionaries {
            storage
                .import_new_dictionary(
                    &DictionaryName::parse(name).unwrap(),
                    records.iter().map(|(headword, definition)| {
                        Ok::<_, anyhow::Error>(entry(headword, definition))
                    }),
                )
                .unwrap();
        }
        (directory, storage)
    }

    #[test]
    fn empty_database_is_a_runtime_error_not_a_miss() {
        let directory = TempDir::new().unwrap();
        let storage = Storage::open_at(directory.path().join("lexi.db")).unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let error = run(
            &storage,
            &[term("hello")],
            &[],
            false,
            &mut stdout,
            &mut stderr,
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("no dictionaries are installed"),
            "{error:#}"
        );
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn global_exact_match_suppresses_case_variants_from_other_dictionaries() {
        let (_directory, storage) = storage_with(&[
            ("oxford", &[("Hello", "oxford Hello")]),
            ("longman", &[("hello", "longman hello")]),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = run(
            &storage,
            &[term("hello")],
            &[],
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(matches!(outcome, Outcome::AllFound));
        assert_eq!(stdout, b"[longman] hello\nlongman hello\n");
        assert!(stderr.is_empty());
    }

    #[test]
    fn case_fallback_keeps_every_folded_candidate() {
        let (_directory, storage) =
            storage_with(&[("oxford", &[("Hello", "first"), ("HELLO", "second")])]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        run(
            &storage,
            &[term("hello")],
            &[],
            false,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(stdout, b"Hello\nfirst\n\nHELLO\nsecond\n");
        assert!(stderr.is_empty());
    }

    #[test]
    fn batch_writes_misses_to_stderr_and_keeps_hits() {
        let (_directory, storage) = storage_with(&[("oxford", &[("hello", "world")])]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = run(
            &storage,
            &[term("hello"), term("helo"), term("hello")],
            &[],
            false,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(matches!(outcome, Outcome::SomeMissing));
        assert_eq!(stdout, b"hello\nworld\n\nhello\nworld\n");
        assert_eq!(stderr, b"No entry found for: helo\n");
    }

    #[test]
    fn select_matches_is_global_not_per_dictionary() {
        let (_directory, storage) = storage_with(&[
            ("oxford", &[("Hello", "oxford")]),
            ("longman", &[("hello", "longman")]),
        ]);
        let scope = storage.resolve_dictionaries(&[]).unwrap();
        let candidates = storage.lookup_folded("hello", &scope).unwrap();
        let selected = select_matches(&term("hello"), candidates);
        assert_eq!(
            selected
                .iter()
                .map(|entry| (entry.dictionary_name(), entry.headword()))
                .collect::<Vec<_>>(),
            vec![("longman", "hello")]
        );
    }
}
