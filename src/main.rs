mod cli;
mod jsonl;
mod record;
mod storage;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use cli::Mode;
use record::DictionaryName;
use storage::Storage;

const EXIT_SUCCESS: u8 = 0;
const EXIT_ARGUMENT_ERROR: u8 = 2;
const EXIT_RUNTIME_ERROR: u8 = 3;

fn main() -> ExitCode {
    let mode = match cli::parse() {
        Ok(mode) => mode,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(EXIT_ARGUMENT_ERROR);
        }
    };

    match run(mode) {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(error) => {
            let stderr = io::stderr();
            let mut stderr = stderr.lock();
            let _ = writeln!(stderr, "Error: {error:#}");
            ExitCode::from(EXIT_RUNTIME_ERROR)
        }
    }
}

fn run(mode: Mode) -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match mode {
        Mode::Help => {
            cli::write_help(&mut stdout).context("failed to write help to stdout")?;
            Ok(())
        }
        Mode::Version => {
            cli::write_version(&mut stdout).context("failed to write version to stdout")?;
            Ok(())
        }
        Mode::List => {
            let storage = Storage::open()?;
            for (name, count) in storage.list_dictionaries()? {
                writeln!(stdout, "{name}\t{count}")
                    .context("failed to write dictionary list to stdout")?;
            }
            Ok(())
        }
        Mode::Query { .. } => bail!("query mode is not implemented yet"),
        Mode::Import { path, name, .. } => {
            let name = DictionaryName::parse(&name)?;
            let records = jsonl::open(&path)?;
            let mut storage = Storage::open()?;
            let count = storage.import_new_dictionary(&name, records)?;
            writeln!(stdout, "Imported {}: {count} entries", name.display())
                .context("failed to write import result to stdout")?;
            Ok(())
        }
        Mode::Remove { .. } => bail!("remove mode is not implemented yet"),
    }
}
