use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};

use crate::record::{DictionaryName, Entry};

pub const SCHEMA_VERSION: i32 = 1;

pub struct Storage {
    connection: Connection,
}

/// Selected dictionaries for a lookup. An empty caller list means every dictionary.
#[derive(Debug)]
pub struct DictionaryScope {
    ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Created(u64),
    Replaced(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEntry {
    dictionary_name: String,
    headword: String,
    definition: String,
}

impl StoredEntry {
    pub fn dictionary_name(&self) -> &str {
        &self.dictionary_name
    }

    pub fn headword(&self) -> &str {
        &self.headword
    }

    pub fn definition(&self) -> &str {
        &self.definition
    }
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

    pub fn has_any_dictionary(&self) -> Result<bool> {
        let exists: i64 = self
            .connection
            .query_row("SELECT EXISTS(SELECT 1 FROM dictionaries)", [], |row| {
                row.get(0)
            })
            .context("failed to check whether any dictionaries are installed")?;
        Ok(exists != 0)
    }

    pub fn resolve_dictionaries(&self, names: &[DictionaryName]) -> Result<DictionaryScope> {
        if names.is_empty() {
            return Ok(DictionaryScope { ids: None });
        }

        let mut ids = Vec::with_capacity(names.len());
        for name in names {
            let id: Option<i64> = self
                .connection
                .query_row(
                    "SELECT id FROM dictionaries WHERE normalized_name = ?1",
                    [name.normalized()],
                    |row| row.get(0),
                )
                .optional()
                .with_context(|| format!("failed to resolve dictionary '{}'", name.display()))?;
            match id {
                Some(id) => ids.push(id),
                None => bail!("unknown dictionary '{}'", name.display()),
            }
        }
        Ok(DictionaryScope { ids: Some(ids) })
    }

    pub fn lookup_folded(
        &self,
        folded_headword: &str,
        scope: &DictionaryScope,
    ) -> Result<Vec<StoredEntry>> {
        match &scope.ids {
            None => collect_lookup(
                &self.connection,
                &lookup_sql(None),
                params![folded_headword],
            ),
            Some(ids) if ids.is_empty() => Ok(Vec::new()),
            Some(ids) => {
                let mut params = Vec::with_capacity(ids.len() + 1);
                params.push(Value::Text(folded_headword.to_owned()));
                params.extend(ids.iter().copied().map(Value::Integer));
                collect_lookup(
                    &self.connection,
                    &lookup_sql(Some(ids.len())),
                    params_from_iter(params),
                )
            }
        }
    }

    pub fn import_dictionary(
        &mut self,
        name: &DictionaryName,
        entries: impl IntoIterator<Item = Result<Entry>>,
        replace_existing: bool,
    ) -> Result<ImportOutcome> {
        // One IMMEDIATE transaction: old-row delete, new inserts, and name/count updates share fate.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("failed to start import transaction")?;

        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT id, name FROM dictionaries WHERE normalized_name = ?1",
                [name.normalized()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("failed to check for an existing dictionary")?;

        let (dictionary_id, replaced) = match existing {
            Some((_id, existing_name)) if !replace_existing => {
                bail!("dictionary '{existing_name}' already exists");
            }
            Some((id, _)) => {
                tx.execute("DELETE FROM entries WHERE dictionary_id = ?1", [id])
                    .with_context(|| {
                        format!(
                            "failed to remove existing entries for dictionary '{}'",
                            name.display()
                        )
                    })?;
                (id, true)
            }
            None => {
                tx.execute(
                    "INSERT INTO dictionaries (name, normalized_name, entry_count) VALUES (?1, ?2, 0)",
                    params![name.display(), name.normalized()],
                )
                .with_context(|| format!("failed to create dictionary '{}'", name.display()))?;
                (tx.last_insert_rowid(), false)
            }
        };

        let entry_count = insert_entries(&tx, dictionary_id, name, entries)?;
        tx.execute(
            "UPDATE dictionaries SET name = ?1, entry_count = ?2 WHERE id = ?3",
            params![name.display(), entry_count as i64, dictionary_id],
        )
        .context("failed to update imported dictionary name and entry count")?;
        tx.commit().context("failed to commit import transaction")?;
        Ok(if replaced {
            ImportOutcome::Replaced(entry_count)
        } else {
            ImportOutcome::Created(entry_count)
        })
    }

    pub fn remove_dictionary(&mut self, name: &DictionaryName) -> Result<(String, u64)> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("failed to start remove transaction")?;

        let existing: Option<(String, u64)> = tx
            .query_row(
                "SELECT name, entry_count FROM dictionaries WHERE normalized_name = ?1",
                [name.normalized()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .with_context(|| format!("failed to look up dictionary '{}'", name.display()))?;

        let Some((original_name, entry_count)) = existing else {
            bail!("unknown dictionary '{}'", name.display());
        };

        tx.execute(
            "DELETE FROM dictionaries WHERE normalized_name = ?1",
            [name.normalized()],
        )
        .with_context(|| format!("failed to remove dictionary '{original_name}'"))?;
        tx.commit()
            .with_context(|| format!("failed to commit removal of dictionary '{original_name}'"))?;
        Ok((original_name, entry_count))
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

fn lookup_sql(dictionary_count: Option<usize>) -> String {
    let mut sql = String::from(
        "SELECT d.name, e.headword, e.definition \
         FROM entries e \
         JOIN dictionaries d ON d.id = e.dictionary_id \
         WHERE e.folded_headword = ?1",
    );
    if let Some(count) = dictionary_count {
        sql.push_str(" AND e.dictionary_id IN (");
        sql.push_str(
            &(2..=count + 1)
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push(')');
    }
    sql.push_str(" ORDER BY e.id");
    sql
}

fn collect_lookup(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<StoredEntry>> {
    let mut statement = connection
        .prepare(sql)
        .context("failed to prepare entry lookup")?;
    let rows = statement
        .query_map(params, |row| {
            Ok(StoredEntry {
                dictionary_name: row.get(0)?,
                headword: row.get(1)?,
                definition: row.get(2)?,
            })
        })
        .context("failed to look up entries")?;
    rows.collect::<rusqlite::Result<_>>()
        .context("failed to read lookup results")
}

fn insert_entries(
    tx: &rusqlite::Transaction<'_>,
    dictionary_id: i64,
    name: &DictionaryName,
    entries: impl IntoIterator<Item = Result<Entry>>,
) -> Result<u64> {
    // One prepared INSERT for the whole file; import memory stays tied to the
    // current JSONL line rather than growing with file size.
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
    Ok(count)
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

    use super::{ImportOutcome, SCHEMA_VERSION, Storage};
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

    fn import_ok(
        storage: &mut Storage,
        name: &str,
        records: impl IntoIterator<Item = Entry>,
        replace_existing: bool,
    ) -> ImportOutcome {
        storage
            .import_dictionary(
                &DictionaryName::parse(name).unwrap(),
                records.into_iter().map(Ok),
                replace_existing,
            )
            .unwrap()
    }

    fn folded_lookup(storage: &Storage, folded: &str) -> Vec<(String, String, String)> {
        let scope = storage.resolve_dictionaries(&[]).unwrap();
        storage
            .lookup_folded(folded, &scope)
            .unwrap()
            .into_iter()
            .map(|entry| {
                (
                    entry.dictionary_name().to_string(),
                    entry.headword().to_string(),
                    entry.definition().to_string(),
                )
            })
            .collect()
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
        let outcome = import_ok(
            &mut storage,
            "  Oxford  ",
            [
                entry("hello", "first"),
                entry("hello", "second"),
                entry("take off", "phrasal"),
            ],
            false,
        );

        assert_eq!(outcome, ImportOutcome::Created(3));
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
        import_ok(&mut storage, "Oxford", [entry("a", "one")], false);

        let error = storage
            .import_dictionary(
                &DictionaryName::parse("oxford").unwrap(),
                [Ok(entry("b", "two"))],
                false,
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
        import_ok(&mut storage, "keep", [entry("safe", "yes")], false);

        let entries: [Result<Entry>; 3] = [
            Ok(entry("one", "1")),
            Ok(entry("two", "2")),
            Err(anyhow!("invalid JSONL record in doomed.jsonl at line 3")),
        ];
        let error = storage
            .import_dictionary(&DictionaryName::parse("doomed").unwrap(), entries, false)
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

    fn explain_lookup(storage: &Storage, sql: &str, params: impl rusqlite::Params) -> String {
        let mut statement = storage
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        statement
            .query_map(params, |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n")
    }

    fn assert_lookup_uses_index(plan: &str) {
        assert!(
            plan.contains("USING INDEX entries_lookup"),
            "lookup did not use entries_lookup:\n{plan}"
        );
        for line in plan.lines() {
            let lower = line.to_ascii_lowercase();
            let scans_entries = (lower.contains("scan e") || lower.contains("scan entries"))
                && !lower.contains("using index");
            assert!(
                !scans_entries,
                "lookup scanned entries without the index:\n{plan}"
            );
        }
    }

    #[test]
    fn lookup_returns_duplicates_across_dictionaries_in_primary_key_order() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "oxford",
            [entry("Hello", "1"), entry("hello", "2")],
            false,
        );
        import_ok(&mut storage, "longman", [entry("HELLO", "3")], false);

        let scope = storage.resolve_dictionaries(&[]).unwrap();
        let found = storage.lookup_folded("hello", &scope).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|entry| {
                    (
                        entry.dictionary_name(),
                        entry.headword(),
                        entry.definition(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("oxford", "Hello", "1"),
                ("oxford", "hello", "2"),
                ("longman", "HELLO", "3"),
            ]
        );
    }

    #[test]
    fn lookup_filter_is_case_insensitive_and_does_not_multiply_repeated_names() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(&mut storage, "Oxford", [entry("hello", "oxford")], false);
        import_ok(&mut storage, "longman", [entry("hello", "longman")], false);

        let names = [
            DictionaryName::parse("OXFORD").unwrap(),
            DictionaryName::parse("oxford").unwrap(),
        ];
        let scope = storage.resolve_dictionaries(&names).unwrap();
        let found = storage.lookup_folded("hello", &scope).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|entry| (entry.dictionary_name(), entry.definition()))
                .collect::<Vec<_>>(),
            vec![("Oxford", "oxford")]
        );
    }

    #[test]
    fn resolve_dictionaries_rejects_unknown_names() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(&mut storage, "oxford", [entry("hello", "def")], false);

        let error = storage
            .resolve_dictionaries(&[DictionaryName::parse("missing").unwrap()])
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("unknown dictionary 'missing'"),
            "{error:#}"
        );
    }

    #[test]
    fn lookup_query_plan_uses_the_folded_headword_index() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(&mut storage, "oxford", [entry("hello", "def")], false);
        import_ok(&mut storage, "longman", [entry("hello", "other")], false);

        let unfiltered = explain_lookup(
            &storage,
            &super::lookup_sql(None),
            rusqlite::params!["hello"],
        );
        assert_lookup_uses_index(&unfiltered);

        let filtered = explain_lookup(
            &storage,
            &super::lookup_sql(Some(2)),
            rusqlite::params!["hello", 1i64, 2i64],
        );
        assert_lookup_uses_index(&filtered);
    }

    #[test]
    fn force_create_when_missing_is_a_normal_import() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();

        let outcome = import_ok(&mut storage, "oxford", [entry("a", "one")], true);

        assert_eq!(outcome, ImportOutcome::Created(1));
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("oxford".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![("a".into(), "one".into(), 1)]
        );
    }

    #[test]
    fn replace_overwrites_name_count_records_and_lookup_order() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "Oxford",
            [entry("old", "first"), entry("old", "second")],
            false,
        );
        import_ok(&mut storage, "keep", [entry("safe", "yes")], false);

        let outcome = import_ok(
            &mut storage,
            "oxford",
            [
                entry("hello", "new one"),
                entry("hello", "new two"),
                entry("world", "three"),
            ],
            true,
        );

        assert_eq!(outcome, ImportOutcome::Replaced(3));
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("oxford".into(), 3), ("keep".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![
                ("safe".into(), "yes".into(), 1),
                ("hello".into(), "new one".into(), 1),
                ("hello".into(), "new two".into(), 2),
                ("world".into(), "three".into(), 3),
            ]
        );
        assert!(folded_lookup(&storage, "old").is_empty());
        assert_eq!(
            folded_lookup(&storage, "hello"),
            vec![
                ("oxford".into(), "hello".into(), "new one".into()),
                ("oxford".into(), "hello".into(), "new two".into()),
            ]
        );
        assert_eq!(
            folded_lookup(&storage, "safe"),
            vec![("keep".into(), "safe".into(), "yes".into())]
        );
    }

    #[test]
    fn replace_matches_normalized_names_case_insensitively() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(&mut storage, "Oxford", [entry("Hello", "old")], false);

        let outcome = import_ok(&mut storage, "OXFORD", [entry("hello", "new")], true);

        assert_eq!(outcome, ImportOutcome::Replaced(1));
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("OXFORD".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![("hello".into(), "new".into(), 1)]
        );
    }

    #[test]
    fn replace_rolls_back_iterator_failure_and_keeps_old_lookup() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "Oxford",
            [entry("old", "first"), entry("old", "second")],
            false,
        );
        import_ok(&mut storage, "keep", [entry("safe", "yes")], false);
        let before_list = storage.list_dictionaries().unwrap();
        let before_entries = stored_entries(&storage);
        let before_lookup = folded_lookup(&storage, "old");

        let entries: [Result<Entry>; 3] = [
            Ok(entry("one", "1")),
            Ok(entry("two", "2")),
            Err(anyhow!("failed to read doomed.jsonl at line 3")),
        ];
        let error = storage
            .import_dictionary(&DictionaryName::parse("oxford").unwrap(), entries, true)
            .unwrap_err();

        assert!(format!("{error:#}").contains("at line 3"), "{error:#}");
        assert_eq!(storage.list_dictionaries().unwrap(), before_list);
        assert_eq!(stored_entries(&storage), before_entries);
        assert_eq!(folded_lookup(&storage, "old"), before_lookup);
        assert_eq!(
            before_lookup,
            vec![
                ("Oxford".into(), "old".into(), "first".into()),
                ("Oxford".into(), "old".into(), "second".into()),
            ]
        );
    }

    #[test]
    fn replace_rolls_back_sqlite_insert_failure() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "Oxford",
            [entry("old", "first"), entry("old", "second")],
            false,
        );
        let before_list = storage.list_dictionaries().unwrap();
        let before_entries = stored_entries(&storage);
        let before_lookup = folded_lookup(&storage, "old");

        storage
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_replace BEFORE INSERT ON entries
                 BEGIN
                     SELECT RAISE(ABORT, 'injected sqlite failure');
                 END;",
            )
            .unwrap();

        let error = storage
            .import_dictionary(
                &DictionaryName::parse("oxford").unwrap(),
                [Ok(entry("hello", "new"))],
                true,
            )
            .unwrap_err();

        assert!(
            format!("{error:#}").contains("injected sqlite failure"),
            "{error:#}"
        );
        assert_eq!(storage.list_dictionaries().unwrap(), before_list);
        assert_eq!(stored_entries(&storage), before_entries);
        assert_eq!(folded_lookup(&storage, "old"), before_lookup);
    }

    #[test]
    fn remove_deletes_the_named_dictionary_and_cascades_entries() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "Oxford",
            [entry("hello", "first"), entry("hello", "second")],
            false,
        );
        import_ok(&mut storage, "keep", [entry("safe", "yes")], false);

        let removed = storage
            .remove_dictionary(&DictionaryName::parse("oxford").unwrap())
            .unwrap();

        assert_eq!(removed, ("Oxford".into(), 2));
        assert_eq!(
            storage.list_dictionaries().unwrap(),
            vec![("keep".into(), 1)]
        );
        assert_eq!(
            stored_entries(&storage),
            vec![("safe".into(), "yes".into(), 1)]
        );
        assert!(folded_lookup(&storage, "hello").is_empty());
        assert_eq!(
            folded_lookup(&storage, "safe"),
            vec![("keep".into(), "safe".into(), "yes".into())]
        );
    }

    #[test]
    fn remove_unknown_dictionary_does_not_change_rows() {
        let directory = TempDir::new().unwrap();
        let mut storage = Storage::open_at(database_path(&directory)).unwrap();
        import_ok(
            &mut storage,
            "Oxford",
            [entry("hello", "first"), entry("hello", "second")],
            false,
        );
        let before_list = storage.list_dictionaries().unwrap();
        let before_entries = stored_entries(&storage);
        let before_lookup = folded_lookup(&storage, "hello");

        let error = storage
            .remove_dictionary(&DictionaryName::parse("missing").unwrap())
            .unwrap_err();

        assert!(
            format!("{error:#}").contains("unknown dictionary 'missing'"),
            "{error:#}"
        );
        assert_eq!(storage.list_dictionaries().unwrap(), before_list);
        assert_eq!(stored_entries(&storage), before_entries);
        assert_eq!(folded_lookup(&storage, "hello"), before_lookup);
    }
}
