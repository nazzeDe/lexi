use std::io::Write;

use anyhow::{Context, Result};

use crate::output;
use crate::record::{DictionaryName, QueryTerm};
use crate::storage::Storage;

#[derive(Debug)]
pub enum Outcome {
    AllFound,
    SomeMissing,
}

pub fn run(
    storage: &Storage,
    terms: &[QueryTerm],
    dictionaries: &[DictionaryName],
    options: output::Options,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Result<Outcome> {
    let lookup = storage.lookup(dictionaries)?;

    let mut missing = false;
    let mut first_record = true;
    for term in terms {
        let matches = lookup.find(term)?;
        if matches.is_empty() {
            output::write_miss(stderr, term.text())
                .context("failed to write query diagnostic to stderr")?;
            missing = true;
            continue;
        }
        let multiple_dictionaries = matches.first().is_some_and(|first| {
            matches
                .iter()
                .any(|entry| entry.dictionary_name() != first.dictionary_name())
        });
        let record_options = output::Options {
            show_dictionary: options.show_dictionary || (!options.raw && multiple_dictionaries),
            ..options
        };
        for entry in &matches {
            output::write_record(
                stdout,
                entry.dictionary_name(),
                entry.headword(),
                entry.definition(),
                record_options,
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

#[cfg(test)]
mod tests {
    use super::{Outcome, run};
    use crate::output::Options;
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
                .import_dictionary(
                    &DictionaryName::parse(name).unwrap(),
                    records.iter().map(|(headword, definition)| {
                        Ok::<_, anyhow::Error>(entry(headword, definition))
                    }),
                    false,
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
            Options::default(),
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
    fn batch_writes_misses_to_stderr_and_keeps_hits() {
        let (_directory, storage) = storage_with(&[("oxford", &[("hello", "world")])]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = run(
            &storage,
            &[term("hello"), term("helo"), term("hello")],
            &[],
            Options::default(),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(matches!(outcome, Outcome::SomeMissing));
        assert_eq!(stdout, b"hello\nworld\n\nhello\nworld\n");
        assert_eq!(stderr, b"No entry found for: helo\n");
    }
}
