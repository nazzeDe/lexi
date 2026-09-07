mod support;

use std::fs;

use support::{CliFixture, closed_output, text};

fn import(fixture: &CliFixture, name: &str, contents: impl AsRef<[u8]>) {
    let jsonl = fixture.write_jsonl(&format!("{name}.jsonl"), contents);
    let output = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", name]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

#[test]
fn empty_database_query_is_a_runtime_error_not_ordinary_misses() {
    let fixture = CliFixture::new();
    let output = fixture.run(&[
        "hello",
        "world",
        "--dictionary",
        "oxford",
        "--show-dictionary",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("Error:"));
    assert!(stderr.contains("no dictionaries are installed"));
    assert!(!stderr.contains("No entry found for:"));
}

#[test]
fn empty_headword_after_trim_is_an_argument_error() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    for args in [vec!["  "], vec!["hello", "\t"], vec![""]] {
        let output = fixture.run(&args);
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
    let fixture = CliFixture::new();
    let output = fixture.run(&["hello", "--dictionary", "  "]);

    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("dictionary name must not be empty"));
    assert!(!fixture.database_path().exists());
}

#[test]
fn unknown_dictionary_errors_before_any_results_or_misses() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture.run(&[
        "hello",
        "missing",
        "--dictionary",
        "oxford",
        "--dictionary",
        "nope",
    ]);

    assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("unknown dictionary 'nope'"));
    assert!(!stderr.contains("No entry found for:"));
}

#[test]
fn query_uses_all_dictionaries_in_primary_key_order_without_deduping() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"oxford first\"}\n\
         {\"headword\":\"hello\",\"definition\":\"oxford second\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman\"}\n",
    );

    let output = fixture.run(&["hello"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "[oxford] hello\noxford first\n\n[oxford] hello\noxford second\n\n[longman] hello\nlongman\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn raw_never_adds_automatic_dictionary_names_and_full_does() {
    let fixture = CliFixture::new();
    for name in ["first", "second"] {
        import(
            &fixture,
            name,
            "{\"headword\":\"word\",\"definition\":\"<p>definition</p>\"}\n",
        );
    }
    let raw = fixture.run(&["word", "--raw"]);
    assert_eq!(raw.status.code(), Some(0));
    assert_eq!(
        text(&raw.stdout),
        "word\n<p>definition</p>\n\nword\n<p>definition</p>\n"
    );
    let shown = fixture.run(&["word", "--raw", "--show-dictionary"]);
    assert_eq!(
        text(&shown.stdout),
        "[first] word\n<p>definition</p>\n\n[second] word\n<p>definition</p>\n"
    );
    let full = fixture.run(&["word", "--full"]);
    assert_eq!(full.status.code(), Some(0));
    assert_eq!(
        text(&full.stdout),
        "[first] word\ndefinition\n\n[second] word\ndefinition\n"
    );
}

#[test]
fn structured_modes_and_pipe_output_use_the_same_content_layers() {
    let fixture = CliFixture::new();
    let definition = include_str!("fixtures/post.html");
    let record = serde_json::json!({"headword": "post", "definition": definition});
    import(&fixture, "synthetic", record.to_string());
    let default = fixture.run(&["post"]);
    let full = fixture.run(&["post", "--full"]);
    let raw = fixture.run(&["post", "--raw"]);
    for output in [&default, &full, &raw] {
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.contains(&0x1b));
    }
    assert!(
        text(&default.stdout)
            .ends_with("Omitted: 2 examples and etymology; use --full to show all.\n")
    );
    assert!(text(&full.stdout).contains("Second marker example."));
    assert!(text(&full.stdout).contains("An old source."));
    assert!(!text(&full.stdout).contains("Omitted:"));
    assert_eq!(text(&raw.stdout), format!("post\n{definition}"));
    let narrow_env = fixture
        .command()
        .arg("post")
        .env("COLUMNS", "4")
        .env("TERM", "xterm-256color")
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap();
    assert_eq!(narrow_env.stdout, default.stdout);
}

#[test]
fn batch_provenance_is_decided_per_terms_selected_matches() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "first",
        "{\"headword\":\"one\",\"definition\":\"first\"}\n",
    );
    import(
        &fixture,
        "second",
        "{\"headword\":\"two\",\"definition\":\"second\"}\n",
    );
    let output = fixture.run(&["one", "two"]);
    assert_eq!(text(&output.stdout), "one\nfirst\n\ntwo\nsecond\n");
}

#[test]
fn repeated_dictionary_filters_are_case_insensitive_and_do_not_multiply() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"hello\",\"definition\":\"oxford\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman\"}\n",
    );

    let output = fixture.run(&[
        "hello",
        "--dictionary",
        "OXFORD",
        "--dictionary",
        "oxford",
        "--dictionary",
        "Oxford",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\noxford\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn dictionary_filter_order_does_not_override_primary_key_order() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"bank\",\"definition\":\"n. 银行\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"bank\",\"definition\":\"n. 河岸\"}\n",
    );

    let output = fixture.run(&["bank", "--dictionary", "longman", "--dictionary", "oxford"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "[oxford] bank\nn. 银行\n\n[longman] bank\nn. 河岸\n"
    );
}

#[test]
fn global_exact_filter_does_not_fall_back_per_dictionary() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"Hello\",\"definition\":\"oxford Hello\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"hello\",\"definition\":\"longman hello\"}\n",
    );

    let exact = fixture.run(&["hello"]);
    assert_eq!(exact.status.code(), Some(0), "{}", text(&exact.stderr));
    assert_eq!(text(&exact.stdout), "hello\nlongman hello\n");

    let other_exact = fixture.run(&["Hello"]);
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
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"Hello\",\"definition\":\"first\"}\n\
         {\"headword\":\"HELLO\",\"definition\":\"second\"}\n\
         {\"headword\":\"Wörter\",\"definition\":\"words\"}\n",
    );

    let ascii = fixture.run(&["hello"]);
    assert_eq!(ascii.status.code(), Some(0), "{}", text(&ascii.stderr));
    assert_eq!(text(&ascii.stdout), "Hello\nfirst\n\nHELLO\nsecond\n");

    let unicode = fixture.run(&["wörter"]);
    assert_eq!(unicode.status.code(), Some(0), "{}", text(&unicode.stderr));
    assert_eq!(text(&unicode.stdout), "Wörter\nwords\n");
}

#[test]
fn extra_normalization_does_not_match() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"latin\"}\n\
         {\"headword\":\"café\",\"definition\":\"coffee\"}\n\
         {\"headword\":\"银行\",\"definition\":\"simplified\"}\n",
    );

    let miss_hello = fixture.run(&["ｈｅｌｌｏ"]);
    assert_eq!(miss_hello.status.code(), Some(1));
    assert!(miss_hello.stdout.is_empty());
    assert_eq!(text(&miss_hello.stderr), "No entry found for: ｈｅｌｌｏ\n");

    let cafe = fixture.run(&["cafe"]);
    assert_eq!(cafe.status.code(), Some(1));
    assert!(cafe.stdout.is_empty());
    assert_eq!(text(&cafe.stderr), "No entry found for: cafe\n");

    let traditional = fixture.run(&["銀行"]);
    assert_eq!(traditional.status.code(), Some(1));
    assert!(traditional.stdout.is_empty());
    assert_eq!(text(&traditional.stderr), "No entry found for: 銀行\n");

    let punctuated = fixture.run(&["hello!"]);
    assert_eq!(punctuated.status.code(), Some(1));
    assert!(punctuated.stdout.is_empty());
    assert_eq!(text(&punctuated.stderr), "No entry found for: hello!\n");
}

#[test]
fn batch_query_continues_after_misses_and_exits_one() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n\
         {\"headword\":\"take off\",\"definition\":\"phrasal\"}\n",
    );

    let output = fixture.run(&["hello", "helo", "take off", "missing"]);

    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n\ntake off\nphrasal\n");
    assert_eq!(
        text(&output.stderr),
        "No entry found for: helo\nNo entry found for: missing\n"
    );
}

#[test]
fn query_trims_whitespace_before_exact_match() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture.run(&["  hello  "]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n");
}

#[test]
fn output_bytes_match_the_text_contract() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "Oxford",
        "{\"headword\":\"a\",\"definition\":\"one\"}\n\
         {\"headword\":\"b\",\"definition\":\"two\\nthree\\n\"}\n\
         {\"headword\":\"c\",\"definition\":\"  keep spaces  \"}\n",
    );

    let default = fixture.run(&["a", "b", "c"]);
    assert_eq!(default.status.code(), Some(0), "{}", text(&default.stderr));
    assert_eq!(
        default.stdout,
        b"a\none\n\nb\ntwo\nthree\n\nc\n  keep spaces  \n"
    );

    let shown = fixture.run(&["a", "--show-dictionary"]);
    assert_eq!(shown.status.code(), Some(0), "{}", text(&shown.stderr));
    assert_eq!(shown.stdout, b"[Oxford] a\none\n");
}

#[test]
fn default_query_renders_html_and_raw_keeps_stored_text() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"<span>hello &amp; world</span>\"}\n",
    );

    let readable = fixture.run(&["hello"]);
    assert_eq!(
        readable.status.code(),
        Some(0),
        "{}",
        text(&readable.stderr)
    );
    assert_eq!(text(&readable.stdout), "hello\nhello & world\n");
    assert!(readable.stderr.is_empty());

    let raw = fixture.run(&["hello", "--raw"]);
    assert_eq!(raw.status.code(), Some(0), "{}", text(&raw.stderr));
    assert_eq!(text(&raw.stdout), "hello\n<span>hello &amp; world</span>\n");
    assert!(raw.stderr.is_empty());
}

#[test]
fn duplicate_query_headwords_are_looked_up_independently() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture.run(&["hello", "hello"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n\nhello\nworld\n");
}

#[test]
fn deleting_source_jsonl_does_not_affect_query() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "oxford.jsonl",
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

    let output = fixture.run(&["hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n");
}

#[test]
fn filtered_miss_is_ordinary_not_found() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );
    import(
        &fixture,
        "longman",
        "{\"headword\":\"bank\",\"definition\":\"river\"}\n",
    );

    let output = fixture.run(&["hello", "--dictionary", "longman"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(text(&output.stderr), "No entry found for: hello\n");
}

#[test]
fn closed_stderr_during_a_miss_is_a_runtime_error() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture
        .command()
        .args(["helo"])
        .stderr(closed_output())
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
}

#[test]
fn closed_stdout_during_query_is_a_runtime_error() {
    let fixture = CliFixture::new();
    import(
        &fixture,
        "oxford",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let output = fixture
        .command()
        .args(["hello"])
        .stdout(closed_output())
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(text(&output.stderr).starts_with("Error: failed to write query result to stdout"));
}
