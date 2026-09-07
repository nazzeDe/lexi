mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{CliFixture, text};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn lock_package_dependencies(lock: &str, package: &str) -> Vec<String> {
    let needle = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line != needle {
            continue;
        }
        let mut in_deps = false;
        let mut deps = Vec::new();
        for line in lines.by_ref() {
            if line.starts_with("[[package]]") {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with("dependencies = [") {
                in_deps = true;
                if trimmed.ends_with(']') {
                    break;
                }
                continue;
            }
            if in_deps {
                if trimmed == "]" {
                    break;
                }
                let dep = trimmed.trim_matches(|c| c == '"' || c == ',').to_string();
                if !dep.is_empty() {
                    deps.push(dep);
                }
            }
        }
        return deps;
    }
    panic!("{package} missing from Cargo.lock");
}

fn rust_sources_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_sources_under(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn cargo_manifest_pins_edition_system_sqlite_and_a_sync_model() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("edition = \"2024\""),
        "first version uses Rust 2024 edition"
    );
    let rusqlite_lines: Vec<_> = manifest
        .lines()
        .filter(|line| line.contains("rusqlite"))
        .collect();
    assert!(
        !rusqlite_lines.is_empty(),
        "Cargo.toml must depend on rusqlite"
    );
    for line in rusqlite_lines {
        assert!(
            !line.contains("bundled"),
            "rusqlite must not enable bundled: {line}"
        );
    }

    let lock = fs::read_to_string(manifest_dir().join("Cargo.lock")).unwrap();
    assert!(
        !lock.contains("name = \"tokio\""),
        "first version stays synchronous and blocking"
    );
    let sqlite_sys = lock_package_dependencies(&lock, "libsqlite3-sys");
    assert!(
        sqlite_sys.iter().any(|dep| dep == "pkg-config"),
        "system SQLite should be discovered through pkg-config: {sqlite_sys:?}"
    );
    assert!(
        !sqlite_sys.iter().any(|dep| dep == "cc"),
        "bundled SQLite would compile through cc: {sqlite_sys:?}"
    );
}

#[test]
fn binary_dynamically_links_system_libsqlite3() {
    let output = Command::new("ldd")
        .arg(env!("CARGO_BIN_EXE_lexi"))
        .output()
        .expect("ldd should run on Linux");
    assert!(
        output.status.success(),
        "ldd failed: {}",
        text(&output.stderr)
    );
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("libsqlite3.so"),
        "expected a dynamic system libsqlite3 link, got:\n{stdout}"
    );
}

#[test]
fn readme_documents_build_jsonl_boundary_and_commands() {
    let readme = fs::read_to_string(manifest_dir().join("README.md")).unwrap();
    for needle in [
        "Rust",
        "libsqlite3",
        "bundled",
        "cargo build --release",
        "JSONL",
        "MDX",
        "--import",
        "--name",
        "--force",
        "--list",
        "--remove",
        "--dictionary",
        "--show-dictionary",
        "--raw",
        "--help",
        "--version",
        "XDG_DATA_HOME",
    ] {
        assert!(
            readme.contains(needle),
            "README is missing required documentation: {needle}"
        );
    }
}

#[test]
fn sources_and_tests_do_not_reference_user_dictionary_sources() {
    let forbidden = ["dict", "file"].concat();
    for dir in ["src", "tests"] {
        for path in rust_sources_under(&manifest_dir().join(dir)) {
            let source = fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains(&forbidden),
                "{} must not read, copy, or parse {forbidden}/",
                path.display()
            );
        }
    }
}

#[test]
fn temporary_xdg_end_to_end_import_list_query_and_remove() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"interjection\\n1. 你好；您好\"}\n",
    );

    let imported = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        text(&imported.stderr)
    );
    assert_eq!(text(&imported.stdout), "Imported oxford: 1 entries\n");
    assert!(imported.stderr.is_empty());

    let list = fixture.run(&["--list"]);
    assert_eq!(list.status.code(), Some(0), "{}", text(&list.stderr));
    assert_eq!(text(&list.stdout), "oxford\t1\n");
    assert!(list.stderr.is_empty());

    let query = fixture.run(&["hello"]);
    assert_eq!(query.status.code(), Some(0), "{}", text(&query.stderr));
    assert_eq!(text(&query.stdout), "hello\ninterjection\n1. 你好；您好\n");
    assert!(query.stderr.is_empty());

    let shown = fixture.run(&["hello", "--show-dictionary"]);
    assert_eq!(shown.status.code(), Some(0), "{}", text(&shown.stderr));
    assert_eq!(
        text(&shown.stdout),
        "[oxford] hello\ninterjection\n1. 你好；您好\n"
    );
    assert!(shown.stderr.is_empty());

    let removed = fixture.run(&["--remove", "oxford"]);
    assert_eq!(removed.status.code(), Some(0), "{}", text(&removed.stderr));
    assert_eq!(text(&removed.stdout), "Removed oxford: 1 entries\n");
    assert!(removed.stderr.is_empty());

    let empty = fixture.run(&["--list"]);
    assert_eq!(empty.status.code(), Some(0), "{}", text(&empty.stderr));
    assert!(empty.stdout.is_empty());
    assert!(empty.stderr.is_empty());
}

#[test]
fn moving_the_source_jsonl_still_allows_query() {
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

    let moved = fixture.files().join("elsewhere").join("sample.jsonl");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&jsonl, &moved).unwrap();
    assert!(!jsonl.exists());

    let output = fixture.run(&["hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "hello\nworld\n");
}

#[test]
fn exit_codes_cover_success_miss_argument_and_runtime() {
    let fixture = CliFixture::new();
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        "{\"headword\":\"hello\",\"definition\":\"world\"}\n",
    );

    let help = fixture.run(&["--help"]);
    assert_eq!(help.status.code(), Some(0), "{}", text(&help.stderr));

    let imported = fixture.run(&["--import", jsonl.to_str().unwrap(), "--name", "oxford"]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        text(&imported.stderr)
    );

    let miss = fixture.run(&["helo"]);
    assert_eq!(miss.status.code(), Some(1), "{}", text(&miss.stderr));
    assert_eq!(text(&miss.stderr), "No entry found for: helo\n");

    let conflict = fixture.run(&["hello", "--list"]);
    assert_eq!(
        conflict.status.code(),
        Some(2),
        "{}",
        text(&conflict.stderr)
    );
    assert!(conflict.stdout.is_empty());

    let runtime = fixture.run(&["--remove", "missing"]);
    assert_eq!(runtime.status.code(), Some(3), "{}", text(&runtime.stderr));
    assert!(runtime.stdout.is_empty());
    assert!(text(&runtime.stderr).contains("unknown dictionary 'missing'"));
}
