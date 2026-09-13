use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::project::Project;
use agentsync::render::Env;
use agentsync::style::Style;
use agentsync::{Error, engine_version, paths};
use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(status) => ExitCode::from(status),
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            // Bash decides colour from stdout even for stderr lines; same here.
            eprintln!("{}: {e}", Style::for_stdout().red("Error"));
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<u8, Error> {
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
        Command::List => {
            let project = Project::discover()?;
            let mut out = std::io::stdout().lock();
            cli::list::run(&project, &Style::for_stdout(), &mut out).map(|()| 0)
        }
        Command::Check => {
            let root = project_root()?;
            let env = Env {
                config_path: std::env::var("AGENTSYNC_CONFIG_PATH").ok(),
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
    }
}

/// `REPO_ROOT` as `lib/check.sh` derives it: `AGENTSYNC_REPO_ROOT`, else the
/// working directory, spelled logically.
fn project_root() -> Result<String, Error> {
    let env_root = std::env::var("AGENTSYNC_REPO_ROOT")
        .ok()
        .filter(|root| !root.is_empty());
    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
    let root = paths::logical_root(
        env_root.as_deref(),
        &cwd,
        std::env::var("PWD").ok().as_deref(),
    );
    if !std::path::Path::new(&root).is_dir() {
        return Err(Error::ProjectRootNotFound(PathBuf::from(
            env_root.unwrap_or(root),
        )));
    }
    Ok(root)
}

fn print_version() -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
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
