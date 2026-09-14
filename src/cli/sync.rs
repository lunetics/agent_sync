//! `agentsync sync`: `lib/sync.sh` with its transaction. The render writes the
//! project in place, as Bash does; a failure after the backup restores it.

use std::path::Path;

use crate::interrupt::{self, Interrupt};
use crate::log::{Log, Sink};
use crate::manifest::{self, Manifest};
use crate::paths::{self, Paths};
use crate::render::{self, Run, Selection, Stop};
use crate::session::Session;
use crate::workspace::Workspace;
use crate::{Error, backup, gitignore};

pub const USAGE: &str = "AgentSync Config Sync Script

Usage: sync.sh [OPTIONS]

Real sync runs snapshot every destination they may change and automatically
restore that snapshot if the run fails. Use 'agentsync rollback' to restore a
successful run manually.

Options:
  --only <tools>    Sync only specified tools (comma-separated)
  --skip <tools>    Skip specified tools (comma-separated)
  --profile <name>  Also sync this profile (default: personal + active profiles)
  --dry-run         Show what would be copied without making changes
  --force           Overwrite destination files even if they were edited manually
  --if-stale        Sync only when source changed since the last sync (else no-op)
  --help            Show this help message
";

/// The options `parse_args` accepts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub dry_run: bool,
    pub force: bool,
    pub if_stale: bool,
    pub only: String,
    pub skip: String,
    pub profile: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Args),
    Help,
    Invalid(String),
}

/// `parse_args`.
pub fn parse(args: &[String]) -> Parsed {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |message: &str| match rest.clone().next() {
            Some(next) if !next.starts_with("--") => {
                rest.next();
                Ok(next.clone())
            }
            _ => Err(Parsed::Invalid(message.to_string())),
        };
        match arg.as_str() {
            "--only" => match value("Option --only requires a comma-separated value") {
                Ok(v) => parsed.only = v,
                Err(invalid) => return invalid,
            },
            "--skip" => match value("Option --skip requires a comma-separated value") {
                Ok(v) => parsed.skip = v,
                Err(invalid) => return invalid,
            },
            "--profile" => match value("Option --profile requires a profile name") {
                Ok(v) => parsed.profile = Some(v),
                Err(invalid) => return invalid,
            },
            "--dry-run" => parsed.dry_run = true,
            "--force" => parsed.force = true,
            "--if-stale" => parsed.if_stale = true,
            "--help" | "-h" => return Parsed::Help,
            other => return Parsed::Invalid(format!("Unknown option: {other}")),
        }
    }
    Parsed::Run(parsed)
}

/// The environment `sync.sh` and `backup.sh` read.
#[derive(Default)]
pub struct Env {
    pub render: render::Env,
    pub skip_backup: bool,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}

/// Runs `sync` for the project at `root`, streaming its log to `sink`, and
/// returns the exit status.
pub fn run(root: &str, args: &[String], env: &Env, colors: bool, sink: Sink) -> u8 {
    let mut log = Log::streaming(colors, sink);
    if let Some(project) = paths::ai_dir_enclosing_root(root) {
        log.error(&format!(
            "Refusing to sync from inside the .ai/ directory: {root}"
        ));
        log.info("Run agentsync from the project root (the parent of .ai/):");
        log.info(&format!("  cd \"{project}\" && agentsync sync"));
        return 2;
    }
    let args = match parse(args) {
        Parsed::Run(args) => args,
        Parsed::Help => {
            print_usage(&mut log);
            return 0;
        }
        Parsed::Invalid(message) => {
            log.error(&message);
            print_usage(&mut log);
            return 1;
        }
    };

    let mut s = Session::new(Workspace::on_disk(root), Paths::on_disk(root));
    s.log = log;
    s.dry_run = args.dry_run;
    s.force = args.force;
    let mut tx = Transaction::default();
    let status = match sync(&mut s, &args, env, &mut tx) {
        Ok(()) => 0,
        Err(Stop(status)) => {
            tx.fail(&mut s, env);
            status
        }
    };
    if let Some(mut interrupt) = s.interrupt.take()
        && let Some(sig) = interrupt.received()
    {
        interrupt.resend(sig);
        return interrupt::status(sig);
    }
    status
}

fn print_usage(log: &mut Log) {
    for line in USAGE.lines() {
        log.out(line.to_string());
    }
}

/// `SYNC_BACKUP_PATH` while `SYNC_TRANSACTION_ACTIVE`.
#[derive(Default)]
struct Transaction {
    backup: Option<String>,
    active: bool,
}

impl Transaction {
    /// `_sync_cleanup` after a failed run.
    fn fail(&mut self, s: &mut Session, env: &Env) {
        let Some(backup_path) = self.backup.clone().filter(|_| self.active) else {
            return;
        };
        self.active = false;
        let root = s.paths.root.clone();
        let shown = s.display(&backup_path);
        s.log.warning("Sync failed; restoring pre-sync state...");
        match backup::restore(&root, &backup_path) {
            Ok(()) => {
                s.log.info(&format!("Restored pre-sync state from {shown}"));
                prune(s, env);
            }
            Err(e) => {
                report_backup_error(&mut s.log, &e);
                s.log.error(&format!(
                    "Automatic restore failed. Backup retained at {shown}"
                ));
            }
        }
    }
}

fn report_backup_error(log: &mut Log, error: &Error) {
    match error {
        Error::Backup(message) => log.err(format!("Error: {message}")),
        other => log.err(other.to_string()),
    }
}

fn prune(s: &mut Session, env: &Env) {
    let root = s.paths.root.clone();
    if let Err(e) = backup::prune(
        &root,
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
        backup::Retention::Bounded,
    ) {
        report_backup_error(&mut s.log, &e);
        s.log.warning("Could not prune old AgentSync backups.");
    }
}

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

/// `main` of `lib/sync.sh` after `parse_args`.
fn sync(s: &mut Session, args: &Args, env: &Env, tx: &mut Transaction) -> Result<(), Stop> {
    let selection = Selection {
        only: args.only.clone(),
        skip: args.skip.clone(),
        profile: args.profile.clone(),
    };
    let mut run = render::prepare(s, &env.render, selection)?;
    if args.if_stale && !is_stale(&s.paths.root, &run) {
        return Ok(());
    }
    render::refuse_configless_cleanup(s, &run)?;
    render::check_version_pin(s, &run)?;
    render::banner(s);
    render::setup_overlays(s, &mut run, true)?;
    render::build_catalog(s, &mut run);

    let root = s.paths.root.clone();
    let previous = Manifest::load(&root).map_err(|e| io(s, e))?;
    s.activate_manifest(previous.as_ref().map(Manifest::paths).unwrap_or_default());
    warn_baseline_replacements(s, &run, previous.is_none());
    check_drift(s, previous.as_ref())?;
    start_transaction(s, &run, env, tx)?;
    render::run_passes(s, &mut run)?;
    finalize(s, &run, previous.as_ref(), tx)?;
    if tx.backup.is_some() {
        prune(s, env);
    }
    tx.active = false;
    Ok(())
}

/// `_sync_is_stale`: no manifest, or any source input modified after it.
fn is_stale(root: &str, run: &Run) -> bool {
    let manifest = Path::new(root).join(manifest::REL);
    let Some(since) = std::fs::metadata(&manifest)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|meta| meta.modified().ok())
    else {
        return true;
    };
    let src = format!("{root}/.ai/src");
    let mut roots: Vec<String> = Vec::new();
    if Path::new(&src).is_dir() {
        roots.push(src.clone());
    }
    let profiles_dir = format!("{root}/.ai/profiles");
    if Path::new(&profiles_dir).is_dir() {
        roots.push(profiles_dir);
    }
    if let Some(config) = run.config_path.as_ref().filter(|p| Path::new(p).is_file()) {
        roots.push(config.clone());
    }
    let sources = &run.sources;
    for rel in [
        &sources.agents,
        &sources.rules,
        &sources.skills,
        &sources.commands,
        &sources.subagents,
    ] {
        if rel.is_empty() {
            continue;
        }
        let abs = if rel.starts_with('/') {
            rel.clone()
        } else {
            format!("{root}/{rel}")
        };
        if paths::is_within(&abs, &src) {
            continue;
        }
        if Path::new(&abs).exists() {
            roots.push(abs);
        }
    }
    if roots.is_empty() {
        return true;
    }
    roots.iter().any(|path| any_newer(Path::new(path), since))
}

/// `find <path> -newer <manifest>`: the path or anything below it, links
/// judged by their own time and never followed.
fn any_newer(path: &Path, since: std::time::SystemTime) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.modified().is_ok_and(|modified| modified > since) {
        return true;
    }
    meta.is_dir()
        && std::fs::read_dir(path).is_ok_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|entry| any_newer(&entry.path(), since))
        })
}

/// `_warn_baseline_replacements`.
fn warn_baseline_replacements(s: &mut Session, run: &Run, baseline: bool) {
    if s.dry_run || !baseline {
        return;
    }
    let mut existing = std::collections::BTreeSet::new();
    for abs in &run.backup_targets {
        let Some(rel) = s.paths.to_repo_relative(abs) else {
            continue;
        };
        let path = Path::new(abs);
        if path.is_file() {
            existing.insert(rel);
        } else if path.is_dir() && has_regular_file(path) {
            existing.insert(format!("{rel}/"));
        }
    }
    if existing.is_empty() {
        return;
    }
    s.log.warning(&format!(
        "First sync in this project — regenerating {} path(s) that already exist:",
        existing.len()
    ));
    for rel in existing {
        s.log.err(format!("      {rel}"));
    }
    s.log
        .err("      Content AgentSync did not generate is replaced from .ai/src/.".into());
    s.log.err(
        "      To keep a file instead, restore it with 'agentsync rollback' and run 'agentsync adopt <file>' first."
            .into(),
    );
}

/// `find <dir> -type f | head -n 1` is not empty.
fn has_regular_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.is_file() {
        return true;
    }
    meta.is_dir()
        && std::fs::read_dir(path).is_ok_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|entry| has_regular_file(&entry.path()))
        })
}

/// `_check_drift_or_exit`.
fn check_drift(s: &mut Session, previous: Option<&Manifest>) -> Result<(), Stop> {
    if s.dry_run {
        return Ok(());
    }
    let drift = previous.map(|m| m.drift(&s.paths.root)).unwrap_or_default();
    if drift.is_empty() {
        return Ok(());
    }
    if s.force {
        s.log.warning(&format!(
            "Overwriting {} file(s) with manual edits (--force):",
            drift.len()
        ));
        for rel in drift {
            s.log.err(format!("      {rel}"));
        }
        return Ok(());
    }
    s.log.error(&format!(
        "Manual edits detected in {} destination file(s) since last sync:",
        drift.len()
    ));
    for rel in drift {
        s.log.err(format!("      {rel}"));
    }
    for line in [
        "",
        "  These files would be silently overwritten. Choose one:",
        "    • Move your edits into .ai/src/, then re-run sync",
        "    • If a tool wrote here out of band, run 'agentsync adopt <file>' to pull it into .ai/src/",
        "    • Re-run with --force to discard the edits and rewrite from source",
        "",
    ] {
        s.log.err(line.to_string());
    }
    Err(Stop(1))
}

/// `_start_sync_transaction`.
fn start_transaction(
    s: &mut Session,
    run: &Run,
    env: &Env,
    tx: &mut Transaction,
) -> Result<(), Stop> {
    if s.dry_run || env.skip_backup {
        return Ok(());
    }
    let root = s.paths.root.clone();
    let mut targets = run.backup_targets.clone();
    if run.update_gitignore {
        targets.push(format!("{root}/.gitignore"));
    }
    targets.push(format!("{root}/{}", manifest::REL));
    s.interrupt = Some(Interrupt::arm());
    match backup::create(&root, "sync", &targets, backup::Retention::Bounded) {
        Ok(path) => {
            tx.backup = Some(path);
            tx.active = true;
            Ok(())
        }
        Err(e) => {
            report_backup_error(&mut s.log, &e);
            s.log
                .error("Could not back up sync targets; no files were changed.");
            Err(Stop(1))
        }
    }
}

/// `_finalize_run`.
fn finalize(
    s: &mut Session,
    run: &Run,
    previous: Option<&Manifest>,
    tx: &Transaction,
) -> Result<(), Stop> {
    let root = s.paths.root.clone();
    render::checkpoint(s)?;
    if !s.dry_run && run.update_gitignore {
        let mut ignored = run.gitignore_profile.clone();
        if run.outputs != "committed" {
            ignored.extend(run.gitignore_generated.iter().cloned());
            ignored.push(manifest::REL.to_string());
        }
        let path = Path::new(&root).join(".gitignore");
        if !ignored.is_empty() || gitignore::has_managed_block(&path) {
            s.log.separator();
            s.log.info("Updating .gitignore...");
            gitignore::update(&path, &ignored, &mut s.log).map_err(|e| io(s, e))?;
        }
    }
    render::checkpoint(s)?;
    if !s.dry_run {
        let touched = s.touched().clone();
        manifest::write(&root, previous, &touched, &mut s.log).map_err(|e| io(s, e))?;
    }

    s.log.separator();
    if s.preserved() > 0 {
        s.log.warning(&format!(
            "Preserved {} user-added file(s) not in .ai/src/ — move them into .ai/src/ to manage them, or re-run with --force to prune.",
            s.preserved()
        ));
    }
    if !run.skipped_names.is_empty() {
        s.log
            .info(&format!("Skipped: {}", run.skipped_names.join(", ")));
    }
    let mut summary = format!("Synced {}/{} tools", run.synced, run.total);
    if run.skipped > 0 {
        summary.push_str(&format!(" ({} skipped)", run.skipped));
    }
    if s.dry_run {
        summary.push_str(" (dry-run)");
    }
    if let Some(path) = &tx.backup {
        let shown = s.display(path);
        s.log.info(&format!("Backup: {shown}"));
    }
    s.log.done(&summary);
    s.log.separator();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_parse_like_parse_args() {
        assert_eq!(
            parse(&strings(&[
                "--only",
                "claude,codex",
                "--dry-run",
                "--profile",
                "hub"
            ])),
            Parsed::Run(Args {
                dry_run: true,
                only: "claude,codex".into(),
                profile: Some("hub".into()),
                ..Args::default()
            })
        );
        assert_eq!(
            parse(&strings(&["--skip", "--force"])),
            Parsed::Invalid("Option --skip requires a comma-separated value".into())
        );
        assert_eq!(
            parse(&strings(&["--profile"])),
            Parsed::Invalid("Option --profile requires a profile name".into())
        );
        assert_eq!(parse(&strings(&["--force", "-h", "--bogus"])), Parsed::Help);
        assert_eq!(
            parse(&strings(&["--bogus"])),
            Parsed::Invalid("Unknown option: --bogus".into())
        );
    }

    #[test]
    fn a_selection_matches_whole_slugs_from_either_list() {
        let selection = Selection {
            only: "claude,codex".into(),
            skip: "codex".into(),
            profile: None,
        };
        assert!(selection.includes("claude"));
        assert!(!selection.includes("codex"));
        assert!(!selection.includes("claud"));
        assert!(Selection::default().includes("anything"));
    }
}
