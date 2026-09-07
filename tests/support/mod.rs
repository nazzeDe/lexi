#![allow(
    dead_code,
    reason = "共享 support 在各集成测试 crate 中只使用其 interface 子集"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

pub struct CliFixture {
    data_home: TempDir,
    files: TempDir,
}

impl CliFixture {
    pub fn new() -> Self {
        Self {
            data_home: TempDir::new().unwrap(),
            files: TempDir::new().unwrap(),
        }
    }

    pub fn data_home(&self) -> &Path {
        self.data_home.path()
    }

    pub fn files(&self) -> &Path {
        self.files.path()
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_home().join("lexi/lexi.db")
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lexi"));
        command
            .env("XDG_DATA_HOME", self.data_home())
            .env_remove("HOME")
            .stdin(Stdio::null());
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("lexi should run")
    }

    pub fn write_jsonl(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.files().join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("CLI output should be UTF-8")
}

#[cfg(unix)]
pub fn closed_output() -> Stdio {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let (reader, writer) = UnixStream::pair().unwrap();
    drop(reader);
    Stdio::from(OwnedFd::from(writer))
}
