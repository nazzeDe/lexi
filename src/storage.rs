use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::record::{DictionaryName, Entry};

pub const SCHEMA_VERSION: i32 = 1;

pub struct Storage {
    connection: Connection,
}

impl Storage {
    pub fn open() -> Result<Self> {
        Self::open_at(database_path()?)
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .context("SQLite database path has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create SQLite data directory {}",
                parent.display()
            )
        })?;

        let connection = Connection::open(path)
            .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .context("failed to enable SQLite foreign keys")?;

        let version: i32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .context("failed to read SQLite schema version")?;
        if version > SCHEMA_VERSION {
            bail!(
                "unsupported SQLite schema version {version}; this lexi supports up to {SCHEMA_VERSION}"
            );
        }
        if version == 0 {
            initialize_schema(&connection).context("failed to initialize SQLite schema")?;
        }

        Ok(Self { connection })
    }

    pub fn list_dictionaries(&self) -> Result<Vec<(String, u64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT name, entry_count FROM dictionaries ORDER BY id")
            .context("failed to prepare dictionary list query")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("failed to list dictionaries")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to read dictionary list")
    }

    pub fn import_new_dictionary(
        &mut self,
        name: &DictionaryName,
        entries: impl IntoIterator<Item = Result<Entry>>,
    ) -> Result<u64> {
        // One IMMEDIATE transaction: any record, I/O, or constraint failure rolls back completely.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("failed to start import transaction")?;

        let existing: Option<String> = tx
            .query_row(
                "SELECT name FROM dictionaries WHERE normalized_name = ?1",
                [name.normalized()],
                |row| row.get(0),
            )
            .optional()
            .context("failed to check for an existing dictionary")?;
        if let Some(existing) = existing {
            bail!("dictionary '{existing}' already exists");
        }

        tx.execute(
            "INSERT INTO dictionaries (name, normalized_name, entry_count) VALUES (?1, ?2, 0)",
            params![name.display(), name.normalized()],
        )
        .with_context(|| format!("failed to create dictionary '{}'", name.display()))?;
        let dictionary_id = tx.last_insert_rowid();

        let entry_count = {
            let mut insert = tx
                .prepare(
                    "INSERT INTO entries (
                         dictionary_id, sequence, headword, folded_headword, definition
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .context("failed to prepare entry insert")?;
            let mut count = 0u64;
            for entry in entries {
                let entry = entry?;
                count += 1;
                insert
                    .execute(params![
                        dictionary_id,
                        count as i64,
                        entry.headword(),
                        entry.folded_headword(),
                        entry.definition(),
                    ])
                    .with_context(|| {
                        format!(
                            "failed to insert entry {count} into dictionary '{}'",
                            name.display()
                        )
                    })?;
            }
            count
        };

        tx.execute(
            "UPDATE dictionaries SET entry_count = ?1 WHERE id = ?2",
            params![entry_count as i64, dictionary_id],
        )
        .context("failed to update imported entry count")?;
        tx.commit().context("failed to commit import transaction")?;
        Ok(entry_count)
    }
}

pub fn database_path() -> Result<PathBuf> {
    let data_home = match env::var_os("XDG_DATA_HOME") {
        Some(path) if !path.is_empty() && Path::new(&path).is_absolute() => PathBuf::from(path),
        _ => {
            let home = env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .context("cannot determine data directory: HOME is not set")?;
            PathBuf::from(home).join(".local/share")
        }
    };
    Ok(data_home.join("lexi/lexi.db"))
}

fn initialize_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(&format!(
        "BEGIN IMMEDIATE;
         CREATE TABLE dictionaries (
             id              INTEGER PRIMARY KEY,
             name            TEXT NOT NULL,
             normalized_name TEXT NOT NULL UNIQUE,
             entry_count     INTEGER NOT NULL
         );
         CREATE TABLE entries (
             id              INTEGER PRIMARY KEY,
             dictionary_id   INTEGER NOT NULL REFERENCES dictionaries(id) ON DELETE CASCADE,
             sequence        INTEGER NOT NULL,
             headword        TEXT NOT NULL,
             folded_headword TEXT NOT NULL,
             definition      TEXT NOT NULL,
             UNIQUE (dictionary_id, sequence)
         );
         CREATE INDEX entries_lookup
             ON entries(folded_headword, dictionary_id, sequence);
         PRAGMA user_version = {SCHEMA_VERSION};
         COMMIT;"
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};
    use rusqlite::{Connection, params};
    use tempfile::TempDir;

    use super::{SCHEMA_VERSION, Storage};
    use crate::record::{DictionaryName, Entry};

    fn database_path(directory: &TempDir) -> std::path::PathBuf {
        directory.path().join("nested/data/lexi.db")
    }

    fn entry(headword: &str, definition: &str) -> Entry {
        Entry::new(headword.to_string(), definition.to_string()).unwrap()
    }

    fn stored_entries(storage: &Storage) -> Vec<(String, String, i64)> {
        storage
            .connection
            .prepare("SELECT headword, definition, sequence FROM entries ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn open_creates_parent_directories_and_versioned_schema() {
        let directory = TempDir::new().unwrap();
        let path = database_path(&directory);

        let storage = Storage::open_at(&path).unwrap();

        assert!(path.is_file());
        let version: i32 = storage
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let tables: Vec<String> = storage
            .connection
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(tables, ["dictionaries", "entries"]);
    }

    #[test]
    fn open_enables_foreign_keys_and_cascades_dictionary_deletion() {
        let directory = TempDir::new().unwrap();
        let storage = Storage::open_at(database_path(&directory)).unwrap();

        let foreign_keys: i32 = storage
            .connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        let invalid_entry = storage.connection.execute(
            "INSERT INTO entries (dictionary_id, sequence, headword, folded_headword, definition)
             VALUES (999, 1, 'word', 'word', 'definition')",
            [],
        );
        assert!(invalid_entry.is_err());

        storage
            .connection
            .execute(
                "INSERT INTO dictionaries (name, normalized_name, entry_count) VALUES (?1, ?2, 1)",
                params!["Oxford", "oxford"],
            )
            .unwrap();
        let dictionary_id = storage.connection.last_insert_rowid();
        storage
            .connection
            .execute(
                "INSERT INTO entries (dictionary_id, sequence, headword, folded_headword, definition)
                 VALUES (?1, 1, 'word', 'word', 'definition')",
                [dictionary_id],
            )
            .unwrap();
        storage
            .connection
            .execute("DELETE FROM dictionaries WHERE id = ?1", [dictionary_id])
            .unwrap();
        let entry_count: i64 = storage
            .connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(entry_count, 0);
    }

    #[test]
    fn schema_preserves_duplicate_headwords_and_indexes_folded_lookup() {
        let directory = TempDir::new().unwrap();
        let storage = Storage::open_at(database_path(&directory)).unwrap();
        storage
            .connection
            .execute(
                "INSERT INTO dictionaries (name, normalized_name, entry_count) VALUES ('test', 'test', 2)",
                [],
            )
            .unwrap();
        let dictionary_id = storage.connection.last_insert_rowid();

        for sequence in [1, 2] {
            storage
                .connection
                .execute(
                    "INSERT INTO entries (dictionary_id, sequence, headword, folded_headword, definition)
                     VALUES (?1, ?2, 'Word', 'word', 'definition')",
                    params![dictionary_id, sequence],
                )
                .unwrap();
        }

        let duplicate_count: i64 = storage
            .connection
            .query_row(
                "SELECT COUNT(*) FROM entries WHERE headword = 'Word'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(duplicate_count, 2);

        let index_columns: Vec<String> = storage
            .connection
            .prepare("SELECT name FROM pragma_index_info('entries_lookup') ORDER BY seqno")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            index_columns,
            ["folded_headword", "dictionary_id", "sequence"]
        );
    }

    #[test]
    fn open_rejects_unknown_future_schema_versions() {
        let directory = TempDir::new().unwrap();
        let path = database_path(&directory);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let error = Storage::open_at(path)
            .err()
            .expect("future schema must fail");

        assert!(
            format!("{error:#}").contains("unsupported SQLite schema version 2"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn import_persists_duplicates_in_order_and_preserves_display_name() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        let name = DictionaryName::parse("  Oxford  ").unwrap();
        let count = storage
            .import_new_dictionary(
                &name,
                [
                    Ok(entry("hello", "first")),
                    Ok(entry("hello", "second")),
                    Ok(entry("take off", "phrasal")),
                ],
            )
            .unwrap();

        assert_eq!(count, 3);
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("Oxford".into(), 3)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![
                ("hello".into(), "first".into(), 1),
                ("hello".into(), "second".into(), 2),
                ("take off".into(), "phrasal".into(), 3),
            ]
        );
    }

    #[test]
    fn import_rejects_the_same_normalized_name_and_leaves_the_original() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        storage
            .import_new_dictionary(
                &DictionaryName::parse("Oxford").unwrap(),
                [Ok(entry("a", "one"))],
            )
            .unwrap();

        let error = storage
            .import_new_dictionary(
                &DictionaryName::parse("oxford").unwrap(),
                [Ok(entry("b", "two"))],
            )
            .unwrap_err();

        assert!(
            format!("{error:#}").contains("dictionary 'Oxford' already exists"),
            "{error:#}"
        );
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("Oxford".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![("a".into(), "one".into(), 1)]
        );
    }

    #[test]
    fn import_rolls_back_partial_records_on_a_later_failure() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        storage
            .import_new_dictionary(
                &DictionaryName::parse("keep").unwrap(),
                [Ok(entry("safe", "yes"))],
            )
            .unwrap();

        let entries: [Result<Entry>; 3] = [
            Ok(entry("one", "1")),
            Ok(entry("two", "2")),
            Err(anyhow!("invalid JSONL record in doomed.jsonl at line 3")),
        ];
        let error = storage
            .import_new_dictionary(&DictionaryName::parse("doomed").unwrap(), entries)
            .unwrap_err();

        assert!(format!("{error:#}").contains("at line 3"), "{error:#}");
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("keep".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![("safe".into(), "yes".into(), 1)]
        );
    }
}
