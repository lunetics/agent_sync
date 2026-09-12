use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use agentsync::cli::{Cli, Command};
use agentsync::{Error, engine_version};
use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), Error> {
    guard_engine_version()?;
    // The Bash CLI accepts these flags as commands; clap's own version flag is off.
    if matches!(
        args.first().and_then(|a| a.to_str()),
        Some("--version" | "-v")
    ) {
        return print_version();
    }
    let cli = Cli::parse_from(std::iter::once(OsString::from("agentsync")).chain(args));
    match cli.command {
        Command::Version => print_version(),
    }
}

fn print_version() -> Result<(), Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version()).map_err(|e| Error::io("<stdout>", e))
}

/// `bin/agentsync.sh` passes its own VERSION so a binary left behind by an
/// older checkout can never answer for a newer engine.
fn guard_engine_version() -> Result<(), Error> {
    let Some(engine) = std::env::var_os("AGENTSYNC_ENGINE_VERSION") else {
        return Ok(());
    };
    let engine = engine.to_string_lossy().into_owned();
    if engine.is_empty() || engine == engine_version() {
        return Ok(());
    }
    Err(Error::StaleBinary {
        binary: engine_version().to_string(),
        engine,
    })
}
