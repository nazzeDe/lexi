mod support;

use std::fs;
use std::path::Path;

use rusqlite::Connection;

use support::{CliFixture, text};

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

fn entry_rows(fixture: &CliFixture) -> Vec<(String, String, String, i64)> {
    Connection::open(fixture.database_path())
        .unwrap()
        .prepare("SELECT headword, folded_headword, definition, sequence FROM entries ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn imports_legal_jsonl_and_prints_the_contract_count() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"interjection\\n1. 你好；您好\"}\n\
         {\"headword\":\"take off\",\"definition\":\"  keep spaces  \"}\n\
         {\"headword\":\"hello\",\"definition\":\"duplicate\"}\n",
    );

    let output = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "  Oxford  "]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported Oxford: 3 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        dictionary_rows(&fixture),
        vec![("Oxford".into(), "oxford".into(), 3)]
    );
    assert_eq!(
        entry_rows(&fixture),
        vec![
            (
                "hello".into(),
                "hello".into(),
                "interjection\n1. 你好；您好".into(),
                1
            ),
            (
                "take off".into(),
                "take off".into(),
                "  keep spaces  ".into(),
                2
            ),
            ("hello".into(), "hello".into(), "duplicate".into(), 3),
        ]
    );
}

#[test]
fn first_import_with_force_still_creates_a_new_dictionary() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"a\",\"definition\":\"one\"}\n",
    );

    let output = fixture.run(&[
        "--import",
        jsonl.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported oxford: 1 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        dictionary_rows(&fixture),
        vec![("oxford".into(), "oxford".into(), 1)]
    );
    assert_eq!(
        entry_rows(&fixture),
        vec![("a".into(), "a".into(), "one".into(), 1)]
    );
    let query = fixture.run(&["a"]);
    assert_eq!(query.status.code(), Some(0), "{}", text(&query.stderr));
    assert_eq!(text(&query.stdout), "a\none\n");
}

#[test]
fn force_replaces_only_the_matching_dictionary_and_prints_replaced() {
    let fixture = CliFixture::new();
    let original = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"old\",\"definition\":\"keep me\"}\n\
         {\"headword\":\"old\",\"definition\":\"second\"}\n",
    );
    let first = fixture.run(&["--import", original.to_str().unwrap(), "--name", "oxford"]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let neighbor = fixture.write_jsonl(
        "longman.jsonl",
        "{\"headword\":\"old\",\"definition\":\"other dict\"}\n",
    );
    let kept = fixture.run(&["--import", neighbor.to_str().unwrap(), "--name", "longman"]);
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));

    let replacement = fixture.write_jsonl(
        "new.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"new one\"}\n\
         {\"headword\":\"hello\",\"definition\":\"new two\"}\n\
         {\"headword\":\"world\",\"definition\":\"three\"}\n",
    );
    let output = fixture.run(&[
        "--import",
        replacement.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Replaced oxford: 3 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        text(&fixture.run(&["--list"]).stdout),
        "oxford\t3\nlongman\t1\n",
    );
    assert_eq!(
        dictionary_rows(&fixture),
        vec![
            ("oxford".into(), "oxford".into(), 3),
            ("longman".into(), "longman".into(), 1),
        ]
    );
    assert_eq!(
        entry_rows(&fixture),
        vec![
            ("old".into(), "old".into(), "other dict".into(), 1),
            ("hello".into(), "hello".into(), "new one".into(), 1),
            ("hello".into(), "hello".into(), "new two".into(), 2),
            ("world".into(), "world".into(), "three".into(), 3),
        ]
    );

    let missed = fixture.run(&["old", "--dictionary", "oxford"]);
    assert_eq!(missed.status.code(), Some(1), "{}", text(&missed.stderr));
    assert!(text(&missed.stderr).contains("No entry found for: old"));
    let found = fixture.run(&["hello", "--dictionary", "oxford", "--show-dictionary"]);
    assert_eq!(found.status.code(), Some(0), "{}", text(&found.stderr));
    assert_eq!(
        text(&found.stdout),
        "[oxford] hello\nnew one\n\n[oxford] hello\nnew two\n",
    );
}

#[test]
fn force_replaces_a_case_insensitive_name_match() {
    let fixture = CliFixture::new();
    let original = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"Hello\",\"definition\":\"old Oxford\"}\n",
    );
    let first = fixture.run(&["--import", original.to_str().unwrap(), "--name", "Oxford"]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let replacement = fixture.write_jsonl(
        "new.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"new oxford\"}\n",
    );
    let output = fixture.run(&[
        "--import",
        replacement.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Replaced oxford: 1 entries\n");
    assert_eq!(text(&fixture.run(&["--list"]).stdout), "oxford\t1\n");
    assert_eq!(
        dictionary_rows(&fixture),
        vec![("oxford".into(), "oxford".into(), 1)]
    );
    assert_eq!(
        entry_rows(&fixture),
        vec![("hello".into(), "hello".into(), "new oxford".into(), 1)]
    );
    let found = fixture.run(&["hello", "--show-dictionary"]);
    assert_eq!(found.status.code(), Some(0), "{}", text(&found.stderr));
    assert_eq!(text(&found.stdout), "[oxford] hello\nnew oxford\n");
}

#[test]
fn accepts_bom_crlf_and_blank_lines() {
    let fixture = CliFixture::new();
    let mut contents = b"\xEF\xBB\xBF".to_vec();
    contents.extend_from_slice(
        b"{\"headword\":\"a\",\"definition\":\"one\"}\r\n \t \r\n\n{\"headword\":\"b\",\"definition\":\"two\"}\r\n",
    );
    let jsonl = fixture.write_jsonl("sample.jsonl", contents);

    let output = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported oxford: 2 entries\n");
    assert_eq!(
        entry_rows(&fixture)
            .into_iter()
            .map(|(headword, _, definition, sequence)| (headword, definition, sequence))
            .collect::<Vec<_>>(),
        vec![("a".into(), "one".into(), 1), ("b".into(), "two".into(), 2),]
    );
}

#[test]
fn validation_failures_include_path_and_line_and_exit_three() {
    let cases: &[(&[u8], &str, &str)] = &[
        (b"{not json}\n", "at line 1", "key must be a string"),
        (
            b"{\"headword\":\"a\",\"definition\":\"b\",\"extra\":1}\n",
            "at line 1",
            "unknown field `extra`",
        ),
        (
            b"{\"headword\":\"\",\"definition\":\"b\"}\n",
            "at line 1",
            "headword must be a non-empty string",
        ),
        (
            b"\n{\"headword\":\"ok\",\"definition\":\"yes\"}\n{\"headword\":\"bad \",\"definition\":\"no\"}\n",
            "at line 3",
            "leading or trailing whitespace",
        ),
        (
            b"{\"definition\":\"only\"}\n",
            "at line 1",
            "missing field `headword`",
        ),
        (
            b"{\"headword\":\"only\"}\n",
            "at line 1",
            "missing field `definition`",
        ),
        (
            b"{\"headword\":1,\"definition\":\"b\"}\n",
            "at line 1",
            "invalid type",
        ),
        (
            b"{\"headword\":\"a\",\"definition\":\"\"}\n",
            "at line 1",
            "definition must be a non-whitespace string",
        ),
        (
            b"{\"headword\":\"a\\tb\",\"definition\":\"b\"}\n",
            "at line 1",
            "control characters",
        ),
    ];

    for (contents, line, reason) in cases {
        let fixture = CliFixture::new();
        let jsonl = fixture.write_jsonl("sample.jsonl", *contents);
        let output = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);

        assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
        assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with("Error:"), "{stderr}");
        assert!(stderr.contains(&jsonl.display().to_string()), "{stderr}");
        assert!(stderr.contains(line), "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
        if fixture.database_path().exists() {
            assert!(dictionary_rows(&fixture).is_empty());
            assert!(entry_rows(&fixture).is_empty());
        }
    }
}

#[test]
fn same_name_different_case_is_rejected_without_changing_the_original() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"a\",\"definition\":\"one\"}\n{\"headword\":\"b\",\"definition\":\"two\"}\n",
    );
    let first = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "Oxford"]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let second = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);

    assert_eq!(second.status.code(), Some(3), "{}", text(&second.stderr));
    assert!(second.stdout.is_empty());
    assert!(text(&second.stderr).contains("dictionary 'Oxford' already exists"));
    assert_eq!(text(&fixture.run(&["--list"]).stdout), "Oxford\t2\n");
    assert_eq!(
        dictionary_rows(&fixture),
        vec![("Oxford".into(), "oxford".into(), 2)]
    );
}

#[test]
fn failed_import_does_not_leave_a_partial_new_dictionary() {
    let fixture = CliFixture::new();
    let good = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"keep\",\"definition\":\"safe\"}\n",
    );
    let kept = fixture.run(&["--import", good.to_str().unwrap(), "--name", "keep"]);
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));

    let bad_path = fixture.write_jsonl(
        "doomed.jsonl",
        "{\"headword\":\"one\",\"definition\":\"1\"}\n{\"headword\":\"\",\"definition\":\"no\"}\n",
    );
    let failed = fixture.run(&["--import", bad_path.to_str().unwrap(), "--name", "doomed"]);

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(text(&failed.stderr).contains("at line 2"));
    assert_eq!(text(&fixture.run(&["--list"]).stdout), "keep\t1\n");
    assert_eq!(
        entry_rows(&fixture),
        vec![("keep".into(), "keep".into(), "safe".into(), 1)]
    );
}

#[test]
fn deleting_the_source_jsonl_does_not_affect_persisted_data() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    let imported = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        text(&imported.stderr)
    );
    fs::remove_file(&jsonl).unwrap();

    let list = fixture.run(&["--list"]);
    assert_eq!(list.status.code(), Some(0), "{}", text(&list.stderr));
    assert_eq!(text(&list.stdout), "oxford\t1\n");
    assert_eq!(
        entry_rows(&fixture),
        vec![("hello".into(), "hello".into(), "world".into(), 1)]
    );
}

#[test]
fn missing_jsonl_is_a_runtime_error() {
    let fixture = CliFixture::new();
    let missing = fixture.data_home().join("missing.jsonl");
    let output = fixture.run(&["--import", missing.to_str().unwrap(), "--name", "oxford"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("failed to open JSONL file"));
    assert!(text(&output.stderr).contains(&missing.display().to_string()));
    assert!(!fixture.database_path().exists());
}

fn import_named(fixture: &CliFixture, jsonl: &Path, name: &str) {
    let output = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", name]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

fn seed_oxford_and_keep(fixture: &CliFixture) {
    let oxford = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"old\",\"definition\":\"first\"}\n\
         {\"headword\":\"old\",\"definition\":\"second\"}\n",
    );
    import_named(fixture, &oxford, "Oxford");
    let keep = fixture.write_jsonl(
        "keep.jsonl",
        "{\"headword\":\"safe\",\"definition\":\"yes\"}\n",
    );
    import_named(fixture, &keep, "keep");
}

fn assert_original_dictionary_still_usable(fixture: &CliFixture) {
    assert_eq!(
        text(&fixture.run(&["--list"]).stdout),
        "Oxford\t2\nkeep\t1\n",
    );
    assert_eq!(
        dictionary_rows(fixture),
        vec![
            ("Oxford".into(), "oxford".into(), 2),
            ("keep".into(), "keep".into(), 1),
        ]
    );
    assert_eq!(
        entry_rows(fixture),
        vec![
            ("old".into(), "old".into(), "first".into(), 1),
            ("old".into(), "old".into(), "second".into(), 2),
            ("safe".into(), "safe".into(), "yes".into(), 1),
        ]
    );
    let query = fixture.run(&["old", "--dictionary", "Oxford", "--show-dictionary"]);
    assert_eq!(query.status.code(), Some(0), "{}", text(&query.stderr));
    assert_eq!(
        text(&query.stdout),
        "[Oxford] old\nfirst\n\n[Oxford] old\nsecond\n",
    );
}

#[test]
fn jsonl_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let fixture = CliFixture::new();
    seed_oxford_and_keep(&fixture);

    let bad = fixture.write_jsonl(
        "doomed.jsonl",
        "{\"headword\":\"one\",\"definition\":\"1\"}\n{\"headword\":\"\",\"definition\":\"no\"}\n",
    );
    let failed = fixture.run(&[
        "--import",
        bad.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains(&bad.display().to_string()), "{stderr}");
    assert!(stderr.contains("at line 2"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(&fixture);
}

#[test]
fn io_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let fixture = CliFixture::new();
    seed_oxford_and_keep(&fixture);

    let mut contents = b"{\"headword\":\"one\",\"definition\":\"1\"}\n".to_vec();
    contents.extend_from_slice(b"\xff\n");
    let bad = fixture.write_jsonl("broken.jsonl", contents);
    let failed = fixture.run(&[
        "--import",
        bad.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains(&bad.display().to_string()), "{stderr}");
    assert!(stderr.contains("at line 2"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(&fixture);
}

#[test]
fn sqlite_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let fixture = CliFixture::new();
    seed_oxford_and_keep(&fixture);

    Connection::open(fixture.database_path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_replace BEFORE INSERT ON entries
             BEGIN
                 SELECT RAISE(ABORT, 'injected sqlite failure');
             END;",
        )
        .unwrap();

    let replacement = fixture.write_jsonl(
        "new.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"new\"}\n",
    );
    let failed = fixture.run(&[
        "--import",
        replacement.to_str().unwrap(),
        "--name",
        "oxford",
        "--force",
    ]);

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains("injected sqlite failure"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(&fixture);
}
