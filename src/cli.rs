use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, Parser, error::ErrorKind};

#[derive(Debug, Parser)]
#[command(
    name = "lexi",
    version,
    about = "Offline exact dictionary lookup",
    long_about = "Import strict JSONL dictionaries into a local SQLite database and perform predictable exact lookups.",
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub struct Cli {
    #[arg(value_name = "HEADWORD")]
    pub headwords: Vec<String>,

    #[arg(long, value_name = "NAME", action = ArgAction::Append)]
    pub dictionary: Vec<String>,

    #[arg(long)]
    pub show_dictionary: bool,

    #[arg(long)]
    pub raw: bool,

    #[arg(long, value_name = "PATH")]
    pub import: Option<PathBuf>,

    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    #[arg(long)]
    pub force: bool,

    #[arg(long)]
    pub list: bool,

    #[arg(long, value_name = "NAME")]
    pub remove: Option<String>,

    #[arg(short = 'h', long, action = ArgAction::SetTrue, help = "Print help")]
    pub help: bool,

    #[arg(short = 'V', long, action = ArgAction::SetTrue, help = "Print version")]
    pub version: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Help,
    Version,
    Query {
        headwords: Vec<String>,
        dictionaries: Vec<String>,
        show_dictionary: bool,
        raw: bool,
    },
    Import {
        path: PathBuf,
        name: String,
        force: bool,
    },
    List,
    Remove {
        name: String,
    },
}

pub fn parse() -> Result<Mode, clap::Error> {
    parse_from(std::env::args_os())
}

pub fn write_help(writer: &mut impl Write) -> io::Result<()> {
    let mut command = Cli::command();
    command.write_help(&mut *writer)?;
    writeln!(writer)?;
    Ok(())
}

pub fn write_version(writer: &mut impl Write) -> io::Result<()> {
    writeln!(writer, "lexi {}", env!("CARGO_PKG_VERSION"))
}

fn parse_from<I, T>(args: I) -> Result<Mode, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    if args.len() == 1 {
        return Ok(Mode::Help);
    }

    validate(Cli::try_parse_from(args)?)
}

fn validate(cli: Cli) -> Result<Mode, clap::Error> {
    let has_query = !cli.headwords.is_empty();
    let has_import = cli.import.is_some();
    let has_list = cli.list;
    let has_remove = cli.remove.is_some();
    let mode_count = [
        has_query,
        has_import,
        has_list,
        has_remove,
        cli.help,
        cli.version,
    ]
    .into_iter()
    .filter(|active| *active)
    .count();

    if mode_count == 0 {
        return Err(argument_error(
            "one mode is required: HEADWORD, --import, --list, --remove, --help, or --version",
        ));
    }
    if mode_count > 1 {
        return Err(argument_error(
            "query, --import, --list, --remove, --help, and --version modes are mutually exclusive",
        ));
    }

    if cli.help || cli.version {
        reject_if(
            cli.name.is_some()
                || cli.force
                || !cli.dictionary.is_empty()
                || cli.show_dictionary
                || cli.raw,
            "--help and --version do not accept business options",
        )?;
        return Ok(if cli.help { Mode::Help } else { Mode::Version });
    }

    if has_query {
        reject_if(
            cli.name.is_some() || cli.force,
            "--name and --force are only valid with --import",
        )?;
        return Ok(Mode::Query {
            headwords: cli.headwords,
            dictionaries: cli.dictionary,
            show_dictionary: cli.show_dictionary,
            raw: cli.raw,
        });
    }

    if has_import {
        reject_if(
            !cli.dictionary.is_empty() || cli.show_dictionary || cli.raw,
            "--dictionary, --show-dictionary, and --raw are only valid with query mode",
        )?;
        let name = cli
            .name
            .ok_or_else(|| argument_error("--import requires --name <NAME>"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(argument_error(
                "--import requires a non-empty --name <NAME>",
            ));
        }
        return Ok(Mode::Import {
            path: cli.import.expect("import mode was checked"),
            name: name.to_string(),
            force: cli.force,
        });
    }

    if has_list {
        reject_if(
            cli.name.is_some()
                || cli.force
                || !cli.dictionary.is_empty()
                || cli.show_dictionary
                || cli.raw,
            "--list does not accept import or query options",
        )?;
        return Ok(Mode::List);
    }

    reject_if(
        cli.name.is_some()
            || cli.force
            || !cli.dictionary.is_empty()
            || cli.show_dictionary
            || cli.raw,
        "--remove does not accept import or query options",
    )?;
    Ok(Mode::Remove {
        name: cli.remove.expect("remove mode was checked"),
    })
}

fn reject_if(condition: bool, message: &'static str) -> Result<(), clap::Error> {
    if condition {
        Err(argument_error(message))
    } else {
        Ok(())
    }
}

fn argument_error(message: &'static str) -> clap::Error {
    Cli::command().error(ErrorKind::ArgumentConflict, message)
}

#[cfg(test)]
mod tests {
    use super::{Mode, parse_from};

    #[test]
    fn parses_each_mode() {
        assert_eq!(parse_from(["lexi"]).unwrap(), Mode::Help);
        assert_eq!(parse_from(["lexi", "--help"]).unwrap(), Mode::Help);
        assert_eq!(parse_from(["lexi", "--version"]).unwrap(), Mode::Version);
        assert!(matches!(
            parse_from(["lexi", "hello"]).unwrap(),
            Mode::Query {
                raw: false,
                show_dictionary: false,
                ..
            }
        ));
        assert!(matches!(
            parse_from(["lexi", "hello", "--raw", "--show-dictionary"]).unwrap(),
            Mode::Query {
                raw: true,
                show_dictionary: true,
                ..
            }
        ));
        assert!(matches!(
            parse_from(["lexi", "--import", "dict.jsonl", "--name", "  Test Dict  "]).unwrap(),
            Mode::Import { name, force, .. } if name == "Test Dict" && !force
        ));
        assert!(matches!(
            parse_from([
                "lexi",
                "--import",
                "dict.jsonl",
                "--name",
                "oxford",
                "--force",
            ])
            .unwrap(),
            Mode::Import { force: true, .. }
        ));
        assert_eq!(parse_from(["lexi", "--list"]).unwrap(), Mode::List);
        assert!(matches!(
            parse_from(["lexi", "--remove", "test"]).unwrap(),
            Mode::Remove { .. }
        ));
    }

    #[test]
    fn rejects_options_outside_their_modes() {
        for args in [
            vec!["lexi", "hello", "--list"],
            vec!["lexi", "--list", "--force"],
            vec!["lexi", "--remove", "test", "--show-dictionary"],
            vec!["lexi", "--list", "--raw"],
            vec!["lexi", "--import", "dict.jsonl"],
            vec!["lexi", "--import", "dict.jsonl", "--name", "   "],
            vec!["lexi", "--list", "--help"],
            vec!["lexi", "hello", "--version"],
        ] {
            let error = parse_from(args).unwrap_err();
            assert_eq!(error.exit_code(), 2);
        }
    }
}
