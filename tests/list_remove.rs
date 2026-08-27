use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use rusqlite::Connection;
use tempfile::TempDir;

fn run(data_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lexi"))
        .args(args)
        .env("XDG_DATA_HOME", data_home)
        .env_remove("HOME")
        .stdin(Stdio::null())
        .output()
        .expect("lexi should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("CLI output should be UTF-8")
}

fn import(data_home: &Path, files: &Path, name: &str, contents: &str) {
    let path = files.join(format!("{name}.jsonl"));
    fs::write(&path, contents).unwrap();
    let output = run(
        data_home,
        &["--import", path.to_str().unwrap(), "--name", name],
    );
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

fn dictionary_rows(data_home: &Path) -> Vec<(String, String, i64)> {
    Connection::open(data_home.join("lexi/lexi.db"))
        .unwrap()
        .prepare("SELECT name, normalized_name, entry_count FROM dictionaries ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn entry_rows(data_home: &Path) -> Vec<(String, String, i64)> {
    Connection::open(data_home.join("lexi/lexi.db"))
        .unwrap()
        .prepare("SELECT headword, definition, sequence FROM entries ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn lists_original_names_tab_separated_counts_and_duplicate_records() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"other\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"second\"}\n\
         {\"headword\":\"world\",\"definition\":\"third\"}\n",
    );

    let output = run(data_home.path(), &["--list"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "longman\t1\nOxford\t3\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn removes_a_named_dictionary_without_confirmation() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"second\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"keep\",\"definition\":\"safe\"}\n",
    );

    let output = run(data_home.path(), &["--remove", "Oxford"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Removed Oxford: 2 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "longman\t1\n"
    );
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![("longman".into(), "longman".into(), 1)]
    );
    assert_eq!(
        entry_rows(data_home.path()),
        vec![("keep".into(), "safe".into(), 1)]
    );

    let kept = run(data_home.path(), &["keep", "--show-dictionary"]);
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));
    assert_eq!(text(&kept.stdout), "[longman] keep\nsafe\n");

    let missing = run(data_home.path(), &["hello"]);
    assert_eq!(missing.status.code(), Some(1), "{}", text(&missing.stderr));
    assert!(text(&missing.stderr).contains("No entry found for: hello"));
}

#[test]
fn remove_matches_names_case_insensitively_and_prints_the_stored_name() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = run(data_home.path(), &["--remove", "  oXfOrD  "]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Removed Oxford: 1 entries\n");
    assert!(output.stderr.is_empty());
    assert!(dictionary_rows(data_home.path()).is_empty());
    assert!(entry_rows(data_home.path()).is_empty());
}

#[test]
fn unknown_remove_writes_stderr_and_leaves_the_database_unchanged() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    let before_dictionaries = dictionary_rows(data_home.path());
    let before_entries = entry_rows(data_home.path());

    let output = run(data_home.path(), &["--remove", "missing"]);

    assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("Error:"), "{stderr}");
    assert!(stderr.contains("unknown dictionary 'missing'"), "{stderr}");
    assert_eq!(dictionary_rows(data_home.path()), before_dictionaries);
    assert_eq!(entry_rows(data_home.path()), before_entries);
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "Oxford\t1\n"
    );
}

#[test]
fn removing_the_last_dictionary_leaves_an_empty_list() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let removed = run(data_home.path(), &["--remove", "Oxford"]);
    assert_eq!(removed.status.code(), Some(0), "{}", text(&removed.stderr));
    assert_eq!(text(&removed.stdout), "Removed Oxford: 1 entries\n");

    let list = run(data_home.path(), &["--list"]);
    assert_eq!(list.status.code(), Some(0), "{}", text(&list.stderr));
    assert!(list.stdout.is_empty(), "{}", text(&list.stdout));
    assert!(list.stderr.is_empty());
    assert!(dictionary_rows(data_home.path()).is_empty());
    assert!(entry_rows(data_home.path()).is_empty());
}
