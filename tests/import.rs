use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use tempfile::TempDir;

fn run(data_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lexi"))
        .args(args)
        .env("XDG_DATA_HOME", data_home)
        .env_remove("HOME")
        .output()
        .expect("lexi should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("CLI output should be UTF-8")
}

fn write_jsonl(directory: &Path, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = directory.join("sample.jsonl");
    fs::write(&path, contents).unwrap();
    path
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

fn entry_rows(data_home: &Path) -> Vec<(String, String, String, i64)> {
    Connection::open(data_home.join("lexi/lexi.db"))
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
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        files.path(),
        "{\"headword\":\"hello\",\"definition\":\"interjection\\n1. 你好；您好\"}\n\
         {\"headword\":\"take off\",\"definition\":\"  keep spaces  \"}\n\
         {\"headword\":\"hello\",\"definition\":\"duplicate\"}\n",
    );

    let output = run(
        data_home.path(),
        &["--import", jsonl.to_str().unwrap(), "--name", "  Oxford  "],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported Oxford: 3 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![("Oxford".into(), "oxford".into(), 3)]
    );
    assert_eq!(
        entry_rows(data_home.path()),
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
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        files.path(),
        "{\"headword\":\"a\",\"definition\":\"one\"}\n",
    );

    let output = run(
        data_home.path(),
        &[
            "--import",
            jsonl.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported oxford: 1 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![("oxford".into(), "oxford".into(), 1)]
    );
    assert_eq!(
        entry_rows(data_home.path()),
        vec![("a".into(), "a".into(), "one".into(), 1)]
    );
    let query = run(data_home.path(), &["a"]);
    assert_eq!(query.status.code(), Some(0), "{}", text(&query.stderr));
    assert_eq!(text(&query.stdout), "a\none\n");
}

#[test]
fn force_replaces_only_the_matching_dictionary_and_prints_replaced() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let original = write_jsonl(
        files.path(),
        "{\"headword\":\"old\",\"definition\":\"keep me\"}\n\
         {\"headword\":\"old\",\"definition\":\"second\"}\n",
    );
    let first = run(
        data_home.path(),
        &["--import", original.to_str().unwrap(), "--name", "oxford"],
    );
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let neighbor = files.path().join("longman.jsonl");
    fs::write(
        &neighbor,
        "{\"headword\":\"old\",\"definition\":\"other dict\"}\n",
    )
    .unwrap();
    let kept = run(
        data_home.path(),
        &["--import", neighbor.to_str().unwrap(), "--name", "longman"],
    );
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));

    let replacement = files.path().join("new.jsonl");
    fs::write(
        &replacement,
        "{\"headword\":\"hello\",\"definition\":\"new one\"}\n\
         {\"headword\":\"hello\",\"definition\":\"new two\"}\n\
         {\"headword\":\"world\",\"definition\":\"three\"}\n",
    )
    .unwrap();
    let output = run(
        data_home.path(),
        &[
            "--import",
            replacement.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Replaced oxford: 3 entries\n");
    assert!(output.stderr.is_empty());
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "oxford\t3\nlongman\t1\n",
    );
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![
            ("oxford".into(), "oxford".into(), 3),
            ("longman".into(), "longman".into(), 1),
        ]
    );
    assert_eq!(
        entry_rows(data_home.path()),
        vec![
            ("old".into(), "old".into(), "other dict".into(), 1),
            ("hello".into(), "hello".into(), "new one".into(), 1),
            ("hello".into(), "hello".into(), "new two".into(), 2),
            ("world".into(), "world".into(), "three".into(), 3),
        ]
    );

    let missed = run(data_home.path(), &["old", "--dictionary", "oxford"]);
    assert_eq!(missed.status.code(), Some(1), "{}", text(&missed.stderr));
    assert!(text(&missed.stderr).contains("No entry found for: old"));
    let found = run(
        data_home.path(),
        &["hello", "--dictionary", "oxford", "--show-dictionary"],
    );
    assert_eq!(found.status.code(), Some(0), "{}", text(&found.stderr));
    assert_eq!(
        text(&found.stdout),
        "[oxford] hello\nnew one\n\n[oxford] hello\nnew two\n",
    );
}

#[test]
fn force_replaces_a_case_insensitive_name_match() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let original = write_jsonl(
        files.path(),
        "{\"headword\":\"Hello\",\"definition\":\"old Oxford\"}\n",
    );
    let first = run(
        data_home.path(),
        &["--import", original.to_str().unwrap(), "--name", "Oxford"],
    );
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let replacement = files.path().join("new.jsonl");
    fs::write(
        &replacement,
        "{\"headword\":\"hello\",\"definition\":\"new oxford\"}\n",
    )
    .unwrap();
    let output = run(
        data_home.path(),
        &[
            "--import",
            replacement.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Replaced oxford: 1 entries\n");
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "oxford\t1\n",
    );
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![("oxford".into(), "oxford".into(), 1)]
    );
    assert_eq!(
        entry_rows(data_home.path()),
        vec![("hello".into(), "hello".into(), "new oxford".into(), 1)]
    );
    let found = run(data_home.path(), &["hello", "--show-dictionary"]);
    assert_eq!(found.status.code(), Some(0), "{}", text(&found.stderr));
    assert_eq!(text(&found.stdout), "[oxford] hello\nnew oxford\n");
}

#[test]
fn accepts_bom_crlf_and_blank_lines() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let mut contents = b"\xEF\xBB\xBF".to_vec();
    contents.extend_from_slice(
        b"{\"headword\":\"a\",\"definition\":\"one\"}\r\n \t \r\n\n{\"headword\":\"b\",\"definition\":\"two\"}\r\n",
    );
    let jsonl = write_jsonl(files.path(), contents);

    let output = run(
        data_home.path(),
        &["--import", jsonl.to_str().unwrap(), "--name", "oxford"],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Imported oxford: 2 entries\n");
    assert_eq!(
        entry_rows(data_home.path())
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
    ];

    for (contents, line, reason) in cases {
        let data_home = TempDir::new().unwrap();
        let files = TempDir::new().unwrap();
        let jsonl = write_jsonl(files.path(), *contents);
        let output = run(
            data_home.path(),
            &["--import", jsonl.to_str().unwrap(), "--name", "oxford"],
        );

        assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
        assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with("Error:"), "{stderr}");
        assert!(stderr.contains(&jsonl.display().to_string()), "{stderr}");
        assert!(stderr.contains(line), "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
        if data_home.path().join("lexi/lexi.db").exists() {
            assert!(dictionary_rows(data_home.path()).is_empty());
            assert!(entry_rows(data_home.path()).is_empty());
        }
    }
}

#[test]
fn same_name_different_case_is_rejected_without_changing_the_original() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        files.path(),
        "{\"headword\":\"a\",\"definition\":\"one\"}\n{\"headword\":\"b\",\"definition\":\"two\"}\n",
    );
    let first = run(
        data_home.path(),
        &["--import", jsonl.to_str().unwrap(), "--name", "Oxford"],
    );
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));

    let second = run(
        data_home.path(),
        &["--import", jsonl.to_str().unwrap(), "--name", "oxford"],
    );

    assert_eq!(second.status.code(), Some(3), "{}", text(&second.stderr));
    assert!(second.stdout.is_empty());
    assert!(text(&second.stderr).contains("dictionary 'Oxford' already exists"));
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "Oxford\t2\n"
    );
    assert_eq!(
        dictionary_rows(data_home.path()),
        vec![("Oxford".into(), "oxford".into(), 2)]
    );
}

#[test]
fn failed_import_does_not_leave_a_partial_new_dictionary() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let good = write_jsonl(
        files.path(),
        "{\"headword\":\"keep\",\"definition\":\"safe\"}\n",
    );
    let kept = run(
        data_home.path(),
        &["--import", good.to_str().unwrap(), "--name", "keep"],
    );
    assert_eq!(kept.status.code(), Some(0), "{}", text(&kept.stderr));

    let bad_path = files.path().join("doomed.jsonl");
    fs::write(
        &bad_path,
        "{\"headword\":\"one\",\"definition\":\"1\"}\n{\"headword\":\"\",\"definition\":\"no\"}\n",
    )
    .unwrap();
    let failed = run(
        data_home.path(),
        &["--import", bad_path.to_str().unwrap(), "--name", "doomed"],
    );

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(text(&failed.stderr).contains("at line 2"));
    assert_eq!(
        text(&run(data_home.path(), &["--list"]).stdout),
        "keep\t1\n"
    );
    assert_eq!(
        entry_rows(data_home.path()),
        vec![("keep".into(), "keep".into(), "safe".into(), 1)]
    );
}

#[test]
fn deleting_the_source_jsonl_does_not_affect_persisted_data() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        files.path(),
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    let imported = run(
        data_home.path(),
        &["--import", jsonl.to_str().unwrap(), "--name", "oxford"],
    );
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        text(&imported.stderr)
    );
    fs::remove_file(&jsonl).unwrap();

    let list = run(data_home.path(), &["--list"]);
    assert_eq!(list.status.code(), Some(0), "{}", text(&list.stderr));
    assert_eq!(text(&list.stdout), "oxford\t1\n");
    assert_eq!(
        entry_rows(data_home.path()),
        vec![("hello".into(), "hello".into(), "world".into(), 1)]
    );
}

#[test]
fn missing_jsonl_is_a_runtime_error() {
    let data_home = TempDir::new().unwrap();
    let missing = data_home.path().join("missing.jsonl");
    let output = run(
        data_home.path(),
        &["--import", missing.to_str().unwrap(), "--name", "oxford"],
    );

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("failed to open JSONL file"));
    assert!(text(&output.stderr).contains(&missing.display().to_string()));
    assert!(!data_home.path().join("lexi/lexi.db").exists());
}

fn import_named(data_home: &Path, jsonl: &Path, name: &str) {
    let output = run(
        data_home,
        &["--import", jsonl.to_str().unwrap(), "--name", name],
    );
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

fn seed_oxford_and_keep(data_home: &Path, files: &Path) {
    let oxford = write_jsonl(
        files,
        "{\"headword\":\"old\",\"definition\":\"first\"}\n\
         {\"headword\":\"old\",\"definition\":\"second\"}\n",
    );
    import_named(data_home, &oxford, "Oxford");
    let keep = files.join("keep.jsonl");
    fs::write(&keep, "{\"headword\":\"safe\",\"definition\":\"yes\"}\n").unwrap();
    import_named(data_home, &keep, "keep");
}

fn assert_original_dictionary_still_usable(data_home: &Path) {
    assert_eq!(
        text(&run(data_home, &["--list"]).stdout),
        "Oxford\t2\nkeep\t1\n",
    );
    assert_eq!(
        dictionary_rows(data_home),
        vec![
            ("Oxford".into(), "oxford".into(), 2),
            ("keep".into(), "keep".into(), 1),
        ]
    );
    assert_eq!(
        entry_rows(data_home),
        vec![
            ("old".into(), "old".into(), "first".into(), 1),
            ("old".into(), "old".into(), "second".into(), 2),
            ("safe".into(), "safe".into(), "yes".into(), 1),
        ]
    );
    let query = run(
        data_home,
        &["old", "--dictionary", "Oxford", "--show-dictionary"],
    );
    assert_eq!(query.status.code(), Some(0), "{}", text(&query.stderr));
    assert_eq!(
        text(&query.stdout),
        "[Oxford] old\nfirst\n\n[Oxford] old\nsecond\n",
    );
}

#[test]
fn jsonl_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    seed_oxford_and_keep(data_home.path(), files.path());

    let bad = files.path().join("doomed.jsonl");
    fs::write(
        &bad,
        "{\"headword\":\"one\",\"definition\":\"1\"}\n{\"headword\":\"\",\"definition\":\"no\"}\n",
    )
    .unwrap();
    let failed = run(
        data_home.path(),
        &[
            "--import",
            bad.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains(&bad.display().to_string()), "{stderr}");
    assert!(stderr.contains("at line 2"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(data_home.path());
}

#[test]
fn io_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    seed_oxford_and_keep(data_home.path(), files.path());

    let mut contents = b"{\"headword\":\"one\",\"definition\":\"1\"}\n".to_vec();
    contents.extend_from_slice(b"\xff\n");
    let bad = files.path().join("broken.jsonl");
    fs::write(&bad, contents).unwrap();
    let failed = run(
        data_home.path(),
        &[
            "--import",
            bad.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains(&bad.display().to_string()), "{stderr}");
    assert!(stderr.contains("at line 2"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(data_home.path());
}

#[test]
fn sqlite_failure_during_force_replace_leaves_the_old_dictionary_usable() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    seed_oxford_and_keep(data_home.path(), files.path());

    Connection::open(data_home.path().join("lexi/lexi.db"))
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_replace BEFORE INSERT ON entries
             BEGIN
                 SELECT RAISE(ABORT, 'injected sqlite failure');
             END;",
        )
        .unwrap();

    let replacement = files.path().join("new.jsonl");
    fs::write(
        &replacement,
        "{\"headword\":\"hello\",\"definition\":\"new\"}\n",
    )
    .unwrap();
    let failed = run(
        data_home.path(),
        &[
            "--import",
            replacement.to_str().unwrap(),
            "--name",
            "oxford",
            "--force",
        ],
    );

    assert_eq!(failed.status.code(), Some(3), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let stderr = text(&failed.stderr);
    assert!(stderr.contains("injected sqlite failure"), "{stderr}");
    assert!(
        !stderr.contains("already exists"),
        "force must attempt replacement instead of rejecting the name: {stderr}"
    );
    assert_original_dictionary_still_usable(data_home.path());
}
