use std::fs;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

fn closed_output() -> Stdio {
    let (reader, writer) = UnixStream::pair().unwrap();
    drop(reader);
    Stdio::from(OwnedFd::from(writer))
}

fn write_jsonl(directory: &Path, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn import(data_home: &Path, files: &Path, name: &str, contents: impl AsRef<[u8]>) {
    let jsonl = write_jsonl(files, &format!("{name}.jsonl"), contents);
    let output = run(
        data_home,
        &["--import", jsonl.to_str().unwrap(), "--name", name],
    );
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

#[test]
fn empty_database_query_is_a_runtime_error_not_ordinary_misses() {
    let data_home = TempDir::new().unwrap();
    let output = run(
        data_home.path(),
        &[
            "hello",
            "world",
            "--dictionary",
            "oxford",
            "--show-dictionary",
        ],
    );

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("Error:"));
    assert!(stderr.contains("no dictionaries are installed"));
    assert!(!stderr.contains("No entry found for:"));
}

#[test]
fn empty_headword_after_trim_is_an_argument_error() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    for args in [vec!["  "], vec!["hello", "\t"], vec![""]] {
        let output = run(data_home.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "unexpected status for {args:?}: {}",
            text(&output.stderr)
        );
        assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
        assert!(
            text(&output.stderr).contains("query headword must not be empty"),
            "{}",
            text(&output.stderr)
        );
    }
}

#[test]
fn empty_dictionary_filter_is_an_argument_error() {
    let data_home = TempDir::new().unwrap();
    let output = run(data_home.path(), &["hello", "--dictionary", "  "]);

    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("dictionary name must not be empty"));
    assert!(!data_home.path().join("lexi/lexi.db").exists());
}

#[test]
fn unknown_dictionary_errors_before_any_results_or_misses() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = run(
        data_home.path(),
        &[
            "hello",
            "missing",
            "--dictionary",
            "oxford",
            "--dictionary",
            "nope",
        ],
    );

    assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("unknown dictionary 'nope'"));
    assert!(!stderr.contains("No entry found for:"));
}

#[test]
fn query_uses_all_dictionaries_in_primary_key_order_without_deduping() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"oxford first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"oxford second\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman\"}\n",
    );

    let output = run(data_home.path(), &["hello"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "hello\noxford first\n\nhello\noxford second\n\nhello\nlongman\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn repeated_dictionary_filters_are_case_insensitive_and_do_not_multiply() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"oxford\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman\"}\n",
    );

    let output = run(
        data_home.path(),
        &[
            "hello",
            "--dictionary",
            "OXFORD",
            "--dictionary",
            "oxford",
            "--dictionary",
            "Oxford",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\noxford\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn dictionary_filter_order_does_not_override_primary_key_order() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"bank\",\"definition\":\"n. 银行\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"bank\",\"definition\":\"n. 河岸\"}\n",
    );

    let output = run(
        data_home.path(),
        &["bank", "--dictionary", "longman", "--dictionary", "oxford"],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "bank\nn. 银行\n\nbank\nn. 河岸\n");
}

#[test]
fn global_exact_filter_does_not_fall_back_per_dictionary() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"Hello\",\"definition\":\"oxford Hello\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman hello\"}\n",
    );

    let exact = run(data_home.path(), &["hello"]);
    assert_eq!(exact.status.code(), Some(0), "{}", text(&exact.stderr));
    assert_eq!(text(&exact.stdout), "hello\nlongman hello\n");

    let other_exact = run(data_home.path(), &["Hello"]);
    assert_eq!(
        other_exact.status.code(),
        Some(0),
        "{}",
        text(&other_exact.stderr)
    );
    assert_eq!(text(&other_exact.stdout), "Hello\noxford Hello\n");
}

#[test]
fn case_fallback_emits_every_folded_candidate() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"Hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"HELLO\",\"definition\":\"second\"}\n\
         {\"headword\":\"Wörter\",\"definition\":\"words\"}\n",
    );

    let ascii = run(data_home.path(), &["hello"]);
    assert_eq!(ascii.status.code(), Some(0), "{}", text(&ascii.stderr));
    assert_eq!(text(&ascii.stdout), "Hello\nfirst\n\nHELLO\nsecond\n");

    let unicode = run(data_home.path(), &["wörter"]);
    assert_eq!(unicode.status.code(), Some(0), "{}", text(&unicode.stderr));
    assert_eq!(text(&unicode.stdout), "Wörter\nwords\n");
}

#[test]
fn extra_normalization_does_not_match() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"latin\"}\n\
         {\"headword\":\"café\",\"definition\":\"coffee\"}\n\
         {\"headword\":\"银行\",\"definition\":\"simplified\"}\n",
    );

    let miss_hello = run(data_home.path(), &["ｈｅｌｌｏ"]);
    assert_eq!(miss_hello.status.code(), Some(1));
    assert!(miss_hello.stdout.is_empty());
    assert_eq!(text(&miss_hello.stderr), "No entry found for: ｈｅｌｌｏ\n");

    let cafe = run(data_home.path(), &["cafe"]);
    assert_eq!(cafe.status.code(), Some(1));
    assert!(cafe.stdout.is_empty());
    assert_eq!(text(&cafe.stderr), "No entry found for: cafe\n");

    let traditional = run(data_home.path(), &["銀行"]);
    assert_eq!(traditional.status.code(), Some(1));
    assert!(traditional.stdout.is_empty());
    assert_eq!(text(&traditional.stderr), "No entry found for: 銀行\n");

    let punctuated = run(data_home.path(), &["hello!"]);
    assert_eq!(punctuated.status.code(), Some(1));
    assert!(punctuated.stdout.is_empty());
    assert_eq!(text(&punctuated.stderr), "No entry found for: hello!\n");
}

#[test]
fn batch_query_continues_after_misses_and_exits_one() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n\
         {\"headword\":\"take off\",\"definition\":\"phrasal\"}\n",
    );

    let output = run(data_home.path(), &["hello", "helo", "take off", "missing"]);

    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n\ntake off\nphrasal\n");
    assert_eq!(
        text(&output.stderr),
        "No entry found for: helo\nNo entry found for: missing\n"
    );
}

#[test]
fn query_trims_whitespace_before_exact_match() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = run(data_home.path(), &["  hello  "]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n");
}

#[test]
fn output_bytes_match_the_text_contract() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "Oxford",
        "{\"headword\":\"a\",\"definition\":\"one\"}\n\
         {\"headword\":\"b\",\"definition\":\"two\\nthree\\n\"}\n\
         {\"headword\":\"c\",\"definition\":\"  keep spaces  \"}\n",
    );

    let default = run(data_home.path(), &["a", "b", "c"]);
    assert_eq!(default.status.code(), Some(0), "{}", text(&default.stderr));
    assert_eq!(
        default.stdout,
        b"a\none\n\nb\ntwo\nthree\n\nc\n  keep spaces  \n"
    );

    let shown = run(data_home.path(), &["a", "--show-dictionary"]);
    assert_eq!(shown.status.code(), Some(0), "{}", text(&shown.stderr));
    assert_eq!(shown.stdout, b"[Oxford] a\none\n");
}

#[test]
fn duplicate_query_headwords_are_looked_up_independently() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = run(data_home.path(), &["hello", "hello"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n\nhello\nworld\n");
}

#[test]
fn deleting_source_jsonl_does_not_affect_query() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        files.path(),
        "oxford.jsonl",
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

    let output = run(data_home.path(), &["hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n");
}

#[test]
fn filtered_miss_is_ordinary_not_found() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"bank\",\"definition\":\"river\"}\n",
    );

    let output = run(data_home.path(), &["hello", "--dictionary", "longman"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(text(&output.stderr), "No entry found for: hello\n");
}

#[test]
fn closed_stderr_during_a_miss_is_a_runtime_error() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_lexi"))
        .args(["helo"])
        .env("XDG_DATA_HOME", data_home.path())
        .env_remove("HOME")
        .stderr(closed_output())
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
}

#[test]
fn closed_stdout_during_query_is_a_runtime_error() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_lexi"))
        .args(["hello"])
        .env("XDG_DATA_HOME", data_home.path())
        .env_remove("HOME")
        .stdout(closed_output())
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(text(&output.stderr).starts_with("Error: failed to write query result to stdout"));
}

#[test]
fn lookup_query_plan_uses_the_folded_headword_index() {
    let data_home = TempDir::new().unwrap();
    let files = TempDir::new().unwrap();
    import(
        data_home.path(),
        files.path(),
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    import(
        data_home.path(),
        files.path(),
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"other\"}\n",
    );

    let found = run(data_home.path(), &["hello"]);
    assert_eq!(found.status.code(), Some(0), "{}", text(&found.stderr));

    let connection = Connection::open(data_home.path().join("lexi/lexi.db")).unwrap();
    let plans = [
        "SELECT d.name, e.headword, e.definition FROM entries e JOIN dictionaries d ON d.id = e.dictionary_id WHERE e.folded_headword = ?1 ORDER BY e.id",
        "SELECT d.name, e.headword, e.definition FROM entries e JOIN dictionaries d ON d.id = e.dictionary_id WHERE e.folded_headword = ?1 AND e.dictionary_id IN (?2, ?3) ORDER BY e.id",
    ];
    for sql in plans {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let params: Vec<rusqlite::types::Value> = if sql.contains("dictionary_id IN") {
            vec![
                rusqlite::types::Value::Text("hello".into()),
                rusqlite::types::Value::Integer(1),
                rusqlite::types::Value::Integer(2),
            ]
        } else {
            vec![rusqlite::types::Value::Text("hello".into())]
        };
        let plan = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("USING INDEX entries_lookup"),
            "missing entries_lookup in {plan}"
        );
        assert!(
            !plan.to_ascii_lowercase().lines().any(|line| {
                (line.contains("scan e") || line.contains("scan entries"))
                    && !line.contains("using index")
            }),
            "scanned entries without the index: {plan}"
        );
    }
}
