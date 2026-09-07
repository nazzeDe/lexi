mod support;

use std::fs::File;
use std::io::Read;
use std::process::Stdio;

use rustix::fs::{Mode, OFlags};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{OptionalActions, Winsize, tcgetattr, tcsetattr, tcsetwinsize};

use support::{CliFixture, text};

fn tty_query(fixture: &CliFixture, width: u16, no_color: bool, args: &[&str]) -> String {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = rustix::fs::open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .unwrap();
    let mut attributes = tcgetattr(&slave).unwrap();
    attributes.make_raw();
    tcsetattr(&slave, OptionalActions::Now, &attributes).unwrap();
    tcsetwinsize(
        &slave,
        Winsize {
            ws_row: 24,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        },
    )
    .unwrap();
    let mut command = fixture.command();
    command
        .args(args)
        .env("TERM", "xterm-256color")
        .stdout(Stdio::from(slave))
        .stderr(Stdio::piped());
    if no_color {
        command.env("NO_COLOR", "1");
    } else {
        command.env_remove("NO_COLOR");
    }
    let child = command.spawn().unwrap();
    drop(command);
    let mut output = Vec::new();
    let mut master = File::from(master);
    let mut buffer = [0; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => output.extend_from_slice(&buffer[..size]),
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => {
                break;
            }
            Err(error) => panic!("PTY read failed: {error}"),
        }
    }
    let status = child.wait_with_output().unwrap();
    assert!(status.status.success(), "{:?}", status.stderr);
    assert!(status.stderr.is_empty());
    String::from_utf8(output).unwrap()
}

#[test]
fn real_tty_wraps_and_styles_while_no_color_and_raw_are_respected() {
    let fixture = CliFixture::new();
    let definition = include_str!("fixtures/post.html");
    let jsonl = fixture.write_jsonl(
        "sample.jsonl",
        serde_json::json!({"headword": "post", "definition": definition}).to_string(),
    );
    let imported = fixture
        .command()
        .args(["--import", jsonl.to_str().unwrap(), "--name", "sample"])
        .output()
        .unwrap();
    assert!(imported.status.success());
    let colored = tty_query(&fixture, 24, false, &["post"]);
    let plain = tty_query(&fixture, 24, true, &["post"]);
    assert!(colored.contains("\x1b[1m"));
    assert!(colored.contains("\x1b[2m"));
    assert!(colored.contains("\x1b[1m  noun\x1b[0m"));
    assert!(colored.contains("\x1b[1m  verb\x1b[0m"));
    assert!(!plain.contains('\x1b'));
    assert_eq!(
        colored
            .replace("\x1b[1m", "")
            .replace("\x1b[2m", "")
            .replace("\x1b[0m", ""),
        plain
    );
    assert!(
        plain
            .lines()
            .all(|line| textwrap::core::display_width(line) <= 24)
    );
    let pipe = fixture.command().arg("post").output().unwrap();
    let pipe = text(&pipe.stdout);
    assert!(!pipe.contains('\x1b'));
    assert!(pipe.contains("\n     > First marker example.\n       第一个例句。"));
    assert!(pipe.contains("\n     - (rare) A distinct subsense.\n       （罕见）不同的子义项。"));
    assert!(
        pipe.lines()
            .any(|line| textwrap::core::display_width(line) > 24)
    );
    let words = |text: &str| text.split_whitespace().collect::<String>();
    assert_eq!(words(&plain), words(&pipe));
    let narrow = tty_query(&fixture, 2, true, &["post"]);
    assert!(
        narrow
            .lines()
            .all(|line| textwrap::core::display_width(line) <= 2)
    );
    assert_eq!(words(&narrow), words(&pipe));
    assert_eq!(narrow.matches('>').count(), pipe.matches('>').count());
    assert_eq!(narrow.matches('-').count(), pipe.matches('-').count());
    let full = tty_query(&fixture, 24, true, &["post", "--full"]);
    assert!(!full.contains("Omitted:"));
    assert!(full.contains("> Subsense"));
    assert!(
        full.lines()
            .all(|line| textwrap::core::display_width(line) <= 24)
    );
    let raw = tty_query(&fixture, 8, false, &["post", "--raw"]);
    assert_eq!(raw, format!("post\n{definition}"));
}
