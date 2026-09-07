mod support;

use rusqlite::Connection;

use support::{CliFixture, text};

fn import(fixture: &CliFixture, name: &str, contents: &str) {
    let path = fixture.write_jsonl(&format!("{name}.jsonl"), contents);
    let output = fixture.run(&["--import", path.to_str().unwrap(), "--name", name]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

fn dictionary_rows(fixture: &CliFixture) -> Vec<(String, String, i64)> {
    Connection::open(fixture.database_path())
        .unwrap()
        .prepare("SELECT name, normalized_name, entry_count FROM dictionaries ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn entry_rows(fixture: &CliFixture) -> Vec<(String, String, i64)> {
    Connection::open(fixture.database_path())
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
    let fixture = CliFixture::new();
    import(
        &fixture,
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"other\"}\n",
    );
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"second\"}\n\
         {\"headword\":\"world\",\"definition\":\"third\"}\n",
    );

    let output = fixture.run(&["--list"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "longman\t1\nOxford\t3\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn removes_a_named_dictionary_without_confirmation() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"second\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"keep\",\"definition\":\"safe\"}\n",
    );

    let output = fixture.run(&["--remove", "Oxford"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Removed Oxford: 2 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(text(&fixture.run(&["--list"]).stdout), "longman\t1\n");
    assert_eq!(
        dictionary_rows(&fixture),
        vec![("longman".into(), "longman".into(), 1)]
    );
    assert_eq!(
        entry_rows(&fixture),
        vec![("keep".into(), "safe".into(), 1)]
    );

    let kept = fixture.run(&["keep", "--show-dictionary"]);
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));
    assert_eq!(text(&kept.stdout), "[longman] keep\nsafe\n");

    let missing = fixture.run(&["hello"]);
    assert_eq!(missing.status.code(), Some(1), "{}", text(&missing.stderr));
    assert!(text(&missing.stderr).contains("No entry found for: hello"));
}

#[test]
fn remove_matches_names_case_insensitively_and_prints_the_stored_name() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture.run(&["--remove", "  oXfOrD  "]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Removed Oxford: 1 entries\n");
    assert!(output.stderr.is_empty());
    assert!(dictionary_rows(&fixture).is_empty());
    assert!(entry_rows(&fixture).is_empty());
}

#[test]
fn unknown_remove_writes_stderr_and_leaves_the_database_unchanged() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    let before_dictionaries = dictionary_rows(&fixture);
    let before_entries = entry_rows(&fixture);

    let output = fixture.run(&["--remove", "missing"]);

    assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("Error:"), "{stderr}");
    assert!(stderr.contains("unknown dictionary 'missing'"), "{stderr}");
    assert_eq!(dictionary_rows(&fixture), before_dictionaries);
    assert_eq!(entry_rows(&fixture), before_entries);
    assert_eq!(text(&fixture.run(&["--list"]).stdout), "Oxford\t1\n");
}

#[test]
fn removing_the_last_dictionary_leaves_an_empty_list() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let removed = fixture.run(&["--remove", "Oxford"]);
    assert_eq!(removed.status.code(), Some(0), "{}", text(&removed.stderr));
    assert_eq!(text(&removed.stdout), "Removed Oxford: 1 entries\n");

    let list = fixture.run(&["--list"]);
    assert_eq!(list.status.code(), Some(0), "{}", text(&list.stderr));
    assert!(list.stdout.is_empty(), "{}", text(&list.stdout));
    assert!(list.stderr.is_empty());
    assert!(dictionary_rows(&fixture).is_empty());
    assert!(entry_rows(&fixture).is_empty());
}
