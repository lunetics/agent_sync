use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::log::{Sink, Stream};
use agentsync::project::Project;
use agentsync::render::Env;
use agentsync::style::Style;
use agentsync::{Error, engine_version, paths, prompts};
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
    if args.first().and_then(|a| a.to_str()) == Some("dedupe") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
        return cli::dedupe::dedupe(
            &rest,
            &cwd,
            &project_root,
            &Style::for_stdout(),
            prompts::is_tty().then_some(&mut prompts::read_terminal as &mut dyn FnMut() -> String),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("migrate") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let prompt_root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
            Some(root) => root,
            None => {
                let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                paths::logical_root(None, &cwd, var("PWD").as_deref())
            }
        };
        let path_var = var("PATH");
        let mut env = cli::migrate::Env {
            version: engine_version(),
            prompt_root,
            no_clipboard: var("AGENTSYNC_NO_CLIPBOARD").as_deref() == Some("1"),
            stdout_tty: std::io::stdout().is_terminal(),
            interactive: prompts::is_tty(),
            confirm: &mut |question: &str, default_yes: bool| {
                prompts::confirm(question, default_yes)
            },
            copy: &mut |text: &str| cli::migrate::copy_to_clipboard(text, path_var.as_deref()),
        };
        return cli::migrate::migrate(
            &rest,
            &Project::discover,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("add") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let env_root = var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty());
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let root = paths::logical_root(env_root.as_deref(), &cwd, var("PWD").as_deref());
        return cli::add::add(
            &rest,
            &root,
            &Style::for_stdout(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("doctor") {
        let env = cli::doctor::Env {
            version: engine_version(),
            external_roots: var("AGENTSYNC_EXTERNAL_SOURCE_ROOTS"),
        };
        return cli::doctor::doctor(
            &Project::discover,
            &Style::for_stdout(),
            &env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("init") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
        let style = Style::for_stdout();
        let sync_env = sync_env();
        let colors = log_colors();
        let mut sync = |root: &str| cli::sync::run(root, &[], &sync_env, colors, streams());
        let mut confirm =
            |question: &str, default_yes: bool| prompts::confirm(question, default_yes);
        let mut multiselect = |title: &str, options: &[String], preselected: &[String]| {
            prompts::multiselect_on_terminal(title, options, preselected, &style)
        };
        let mut env = cli::init::Env {
            version: engine_version(),
            cwd,
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
            backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            interactive: prompts::is_tty(),
            confirm: &mut confirm,
            multiselect: &mut multiselect,
            sync: &mut sync,
        };
        return cli::init::init(
            &rest,
            &style,
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("refresh") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
            Some(root) => root,
            None => {
                let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                paths::logical_root(None, &cwd, var("PWD").as_deref())
            }
        };
        let mut env = cli::refresh::Env {
            interactive: prompts::is_tty(),
            read_line: &mut prompts::read_terminal,
        };
        return cli::refresh::refresh(
            &rest,
            &root,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("upgrade-config") {
        let root = project_root()?;
        return cli::upgrade_config::run(
            Path::new(&root),
            engine_version(),
            &Style::for_stdout(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    // These parse their arguments as their Bash `cmd_*` do; clap would consume a leading `--`.
    if let Some(
        command @ ("enable" | "disable" | "customize" | "show" | "diff" | "simplify" | "resolve"
        | "profile" | "adopt"),
    ) = args.first().and_then(|a| a.to_str())
    {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let style = Style::for_stdout();
        let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
        return match command {
            "enable" => cli::enable::enable(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |question: &str| prompts::confirm(question, true),
                &mut out,
                &mut err,
            ),
            "disable" => {
                cli::enable::disable(&rest, &Project::discover, &style, &mut out, &mut err)
            }
            "show" => cli::show::show(&rest, &Project::discover, &style, &mut out, &mut err),
            "adopt" => cli::adopt::adopt(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |question: &str| prompts::confirm(question, false),
                &mut out,
                &mut err,
            ),
            "profile" => cli::profile::profile(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |question: &str| prompts::confirm(question, false),
                &mut out,
                &mut err,
            ),
            "diff" => cli::diff::diff(&rest, &Project::discover, &style, &mut out, &mut err),
            "resolve" => cli::resolve::resolve(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |prompt: &str, out: &mut dyn Write| {
                    let _ = write!(out, "        {prompt} ");
                    let _ = out.flush();
                    prompts::read_terminal()
                },
                &mut out,
                &mut err,
            ),
            "simplify" => cli::simplify::simplify(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |prompt: &str, out: &mut dyn Write| {
                    let _ = write!(out, "  {prompt} ");
                    let _ = out.flush();
                    prompts::read_terminal()
                },
                &mut out,
                &mut err,
            ),
            _ => cli::customize::customize(
                &rest,
                &Project::discover,
                &style,
                std::io::stdin().is_terminal(),
                &mut |prompt: &str| {
                    eprint!("{prompt}");
                    let _ = std::io::stderr().flush();
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    line.trim_matches([' ', '\t', '\n']).to_string()
                },
                &mut out,
                &mut err,
            ),
        };
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
                skip_post_sync: Some("true".to_string()),
                allow_post_sync: None,
                backup: None,
                external_source_roots: var("AGENTSYNC_EXTERNAL_SOURCE_ROOTS"),
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
        Command::Sync { args } if args.iter().any(|a| a == "--workspace") => {
            let forwarded: Vec<String> = args.into_iter().filter(|a| a != "--workspace").collect();
            let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
            let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
            Ok(cli::workspace::run(
                &cwd,
                &forwarded,
                &sync_env(),
                &Style::for_stdout(),
                log_colors(),
                &streams,
            ))
        }
        Command::Rollback { args } => {
            let supplied_root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
                Some(root) => root,
                None => {
                    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                    paths::logical_root(None, &cwd, var("PWD").as_deref())
                }
            };
            let env = cli::rollback::Env {
                config_path: var("AGENTSYNC_CONFIG_PATH"),
                backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
                backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            };
            let mut confirm = |question: &str| prompts::confirm(question, false);
            Ok(cli::rollback::run(
                &supplied_root,
                &args,
                &env,
                &mut confirm,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            ))
        }
        Command::Sync { args } => {
            let root = project_root()?;
            Ok(cli::sync::run(
                &root,
                &args,
                &sync_env(),
                log_colors(),
                streams(),
            ))
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn sync_env() -> cli::sync::Env {
    let skip_backup = var("AGENTSYNC_INTERNAL_SKIP_BACKUP").as_deref() == Some("true");
    cli::sync::Env {
        render: Env {
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            skip_post_sync: var("AGENTSYNC_SKIP_POST_SYNC"),
            allow_post_sync: var("AGENTSYNC_ALLOW_POST_SYNC"),
            backup: (!skip_backup).then(|| agentsync::render::BackupBounds {
                limit: var("AGENTSYNC_BACKUP_LIMIT"),
                max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            }),
            external_source_roots: var("AGENTSYNC_EXTERNAL_SOURCE_ROOTS"),
        },
        skip_backup,
        backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
        backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
    }
}

/// `_use_colors` of `logging.sh`: stdout is a terminal and `NO_COLOR` is empty.
fn log_colors() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && var("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Log lines to the process streams as `echo` writes them. A closed stdout does
/// not stop the run: the transaction finishes, as it would with nobody reading.
fn streams() -> Sink {
    Box::new(|stream, line| {
        let _ = match stream {
            Stream::Out => writeln!(std::io::stdout(), "{line}"),
            Stream::Err => writeln!(std::io::stderr(), "{line}"),
        };
    })
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
