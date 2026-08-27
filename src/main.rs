mod cli;
mod jsonl;
mod output;
mod query;
mod record;
mod storage;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use cli::Mode;
use record::{DictionaryName, QueryTerm};
use storage::Storage;

const EXIT_SUCCESS: u8 = 0;
const EXIT_NOT_FOUND: u8 = 1;
const EXIT_ARGUMENT_ERROR: u8 = 2;
const EXIT_RUNTIME_ERROR: u8 = 3;

enum CommandError {
    Argument(anyhow::Error),
    Runtime(anyhow::Error),
}

fn main() -> ExitCode {
    let mode = match cli::parse() {
        Ok(mode) => mode,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(EXIT_ARGUMENT_ERROR);
        }
    };

    match run(mode) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let (code, cause) = match error {
                CommandError::Argument(error) => (EXIT_ARGUMENT_ERROR, error),
                CommandError::Runtime(error) => (EXIT_RUNTIME_ERROR, error),
            };
            let stderr = io::stderr();
            let mut stderr = stderr.lock();
            let _ = writeln!(stderr, "Error: {cause:#}");
            ExitCode::from(code)
        }
    }
}

fn run(mode: Mode) -> Result<u8, CommandError> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match mode {
        Mode::Help => {
            cli::write_help(&mut stdout)
                .context("failed to write help to stdout")
                .map_err(CommandError::Runtime)?;
            Ok(EXIT_SUCCESS)
        }
        Mode::Version => {
            cli::write_version(&mut stdout)
                .context("failed to write version to stdout")
                .map_err(CommandError::Runtime)?;
            Ok(EXIT_SUCCESS)
        }
        Mode::List => {
            let storage = Storage::open().map_err(CommandError::Runtime)?;
            for (name, count) in storage.list_dictionaries().map_err(CommandError::Runtime)? {
                writeln!(stdout, "{name}\t{count}")
                    .context("failed to write dictionary list to stdout")
                    .map_err(CommandError::Runtime)?;
            }
            Ok(EXIT_SUCCESS)
        }
        Mode::Query {
            headwords,
            dictionaries,
            show_dictionary,
        } => {
            let terms = headwords
                .iter()
                .map(|headword| QueryTerm::parse(headword))
                .collect::<Result<Vec<_>>>()
                .map_err(CommandError::Argument)?;
            let dictionaries = dictionaries
                .iter()
                .map(|name| DictionaryName::parse(name))
                .collect::<Result<Vec<_>>>()
                .map_err(CommandError::Argument)?;
            let storage = Storage::open().map_err(CommandError::Runtime)?;
            let stderr = io::stderr();
            let mut stderr = stderr.lock();
            match query::run(
                &storage,
                &terms,
                &dictionaries,
                show_dictionary,
                &mut stdout,
                &mut stderr,
            )
            .map_err(CommandError::Runtime)?
            {
                query::Outcome::AllFound => Ok(EXIT_SUCCESS),
                query::Outcome::SomeMissing => Ok(EXIT_NOT_FOUND),
            }
        }
        Mode::Import { path, name, .. } => {
            let name = DictionaryName::parse(&name).map_err(CommandError::Argument)?;
            let records = jsonl::open(&path).map_err(CommandError::Runtime)?;
            let mut storage = Storage::open().map_err(CommandError::Runtime)?;
            let count = storage
                .import_new_dictionary(&name, records)
                .map_err(CommandError::Runtime)?;
            writeln!(stdout, "Imported {}: {count} entries", name.display())
                .context("failed to write import result to stdout")
                .map_err(CommandError::Runtime)?;
            Ok(EXIT_SUCCESS)
        }
        Mode::Remove { .. } => Err(CommandError::Runtime(anyhow::anyhow!(
            "remove mode is not implemented yet"
        ))),
    }
}
