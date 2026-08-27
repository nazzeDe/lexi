use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
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

#[test]
fn no_arguments_matches_long_help_without_opening_the_database() {
    let data_home = TempDir::new().unwrap();

    let no_args = run(data_home.path(), &[]);
    let help = run(data_home.path(), &["--help"]);

    assert_eq!(no_args.status.code(), Some(0));
    assert_eq!(help.status.code(), Some(0));
    assert_eq!(no_args.stdout, help.stdout);
    assert!(text(&no_args.stdout).contains("Usage: lexi"));
    assert!(text(&no_args.stdout).contains("--import <PATH>"));
    assert!(text(&no_args.stdout).contains("--show-dictionary"));
    assert!(no_args.stderr.is_empty());
    assert!(help.stderr.is_empty());
    assert!(!data_home.path().join("lexi/lexi.db").exists());
}

#[test]
fn long_version_succeeds_without_opening_the_database() {
    let data_home = TempDir::new().unwrap();

    let output = run(data_home.path(), &["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), "lexi 0.1.0\n");
    assert!(output.stderr.is_empty());
    assert!(!data_home.path().join("lexi/lexi.db").exists());
}

#[test]
fn invalid_arguments_and_mode_conflicts_exit_two_on_stderr() {
    let cases = [
        vec!["hello", "--list"],
        vec!["--list", "--force"],
        vec!["--remove", "oxford", "--show-dictionary"],
        vec!["--import", "dict.jsonl"],
        vec!["--import", "dict.jsonl", "--name", "   "],
        vec!["--import", "dict.jsonl", "--name", ""],
        vec!["--name", "oxford"],
        vec!["--list", "--help"],
        vec!["hello", "--version"],
        vec!["--remove"],
        vec!["-l"],
    ];

    for args in cases {
        let data_home = TempDir::new().unwrap();
        let output = run(data_home.path(), &args);

        assert_eq!(
            output.status.code(),
            Some(2),
            "unexpected status for {args:?}: stdout={}, stderr={}",
            text(&output.stdout),
            text(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "stdout was not empty for {args:?}"
        );
        assert!(
            text(&output.stderr).starts_with("error:"),
            "missing argument error for {args:?}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn empty_list_bootstraps_the_xdg_database_and_writes_no_output() {
    let data_home = TempDir::new().unwrap();

    let output = run(data_home.path(), &["--list"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let database = data_home.path().join("lexi/lexi.db");
    assert!(database.is_file());
    let connection = Connection::open(database).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn list_uses_the_home_fallback_when_xdg_data_home_is_unset() {
    let home = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lexi"))
        .arg("--list")
        .env_remove("XDG_DATA_HOME")
        .env("HOME", home.path())
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(home.path().join(".local/share/lexi/lexi.db").is_file());
}

#[test]
fn list_ignores_relative_xdg_data_home_and_uses_home_fallback() {
    let workspace = TempDir::new().unwrap();
    let home = workspace.path().join("home");
    std::fs::create_dir(&home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_lexi"))
        .arg("--list")
        .current_dir(workspace.path())
        .env("XDG_DATA_HOME", "relative-data")
        .env("HOME", &home)
        .output()
        .expect("lexi should run");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(home.join(".local/share/lexi/lexi.db").is_file());
    assert!(!workspace.path().join("relative-data").exists());
}

#[test]
fn closed_stdout_is_a_runtime_error_for_help_version_and_nonempty_list() {
    let data_home = TempDir::new().unwrap();
    let bootstrap = run(data_home.path(), &["--list"]);
    assert_eq!(bootstrap.status.code(), Some(0));
    let database = data_home.path().join("lexi/lexi.db");
    Connection::open(database)
        .unwrap()
        .execute(
            "INSERT INTO dictionaries (name, normalized_name, entry_count) VALUES ('test', 'test', 0)",
            [],
        )
        .unwrap();
    let list = run(data_home.path(), &["--list"]);
    assert_eq!(list.status.code(), Some(0));
    assert_eq!(text(&list.stdout), "test\t0\n");
    assert!(list.stderr.is_empty());

    for args in [&["--help"][..], &["--version"][..], &["--list"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_lexi"))
            .args(args)
            .env("XDG_DATA_HOME", data_home.path())
            .env_remove("HOME")
            .stdout(closed_output())
            .output()
            .expect("lexi should run");

        assert_eq!(
            output.status.code(),
            Some(3),
            "unexpected status for {args:?}"
        );
        assert!(text(&output.stderr).starts_with("Error: failed to write"));
    }
}

#[test]
fn unavailable_stderr_does_not_change_the_runtime_exit_code() {
    let data_home = TempDir::new().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexi"))
        .arg("--version")
        .env("XDG_DATA_HOME", data_home.path())
        .env_remove("HOME")
        .stdout(closed_output())
        .stderr(closed_output())
        .status()
        .expect("lexi should run");

    assert_eq!(status.code(), Some(3));
}

#[test]
fn list_rejects_a_database_from_a_future_schema_version() {
    let data_home = TempDir::new().unwrap();
    let data_dir = data_home.path().join("lexi");
    std::fs::create_dir_all(&data_dir).unwrap();
    let connection = Connection::open(data_dir.join("lexi.db")).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);

    let output = run(data_home.path(), &["--list"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("unsupported SQLite schema version 2"));
}
