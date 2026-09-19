//! `agentsync rollback`: `cmd_rollback` of `lib/helpers/backup.sh`. A safety
//! snapshot is taken first, and a restore that fails or is interrupted puts
//! it back.

use std::io::Write;

use crate::interrupt::{self, Interrupt};
use crate::witness::{self, Preflight};
use crate::{Error, backup, paths, project_config};

pub const USAGE: &str = "Usage: agentsync rollback [<backup-id>] [OPTIONS]

Restore AgentSync-managed targets from a backup. Without an ID, restores the
latest complete snapshot. A safety snapshot is created before every restore,
so the rollback itself can be undone.

Rollback refuses, naming the first changed path, when a target differs from the
state recorded after the backup's operation finished.

Options:
  --list       List complete backups
  --dry-run    Show the restore plan and any conflict without changing files
  --force      Restore even when targets changed after the backup's operation
  -y, --yes    Skip the confirmation prompt
  -h, --help   Show this help
";

/// The environment `backup_configure` and `backup_prune` read.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}

/// Runs `rollback` for the project at `supplied_root`; `confirm` answers the
/// restore question when `--yes` is absent. Returns the exit status.
pub fn run(
    supplied_root: &str,
    args: &[String],
    env: &Env,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut backup_id: Option<String> = None;
    let (mut list_only, mut dry_run, mut assume_yes, mut force) = (false, false, false, false);
    for arg in args {
        match arg.as_str() {
            "--list" => list_only = true,
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--yes" | "-y" => assume_yes = true,
            "--help" | "-h" => {
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            option if option.starts_with('-') => {
                let _ = writeln!(err, "Error: Unknown rollback option: {option}");
                let _ = err.write_all(USAGE.as_bytes());
                return 1;
            }
            id if backup_id.is_some() => {
                let _ = writeln!(err, "Error: Unexpected rollback argument: {id}");
                return 1;
            }
            id => backup_id = Some(id.to_string()),
        }
    }

    let fail = |err: &mut dyn Write, e: Error| {
        let _ = match e {
            Error::Backup(message) => writeln!(err, "Error: {message}"),
            other => writeln!(err, "{other}"),
        };
        1
    };
    let root = match backup::canonical_root(supplied_root) {
        Ok(root) => root,
        Err(e) => return fail(err, e),
    };

    if list_only {
        if backup_id.is_some() {
            let _ = writeln!(err, "Error: A backup ID cannot be combined with --list");
            return 1;
        }
        let rows = match backup::list(&root) {
            Ok(rows) => rows,
            Err(e) => return fail(err, e),
        };
        if rows.is_empty() {
            let _ = writeln!(out, "No AgentSync backups found.");
            return 0;
        }
        let _ = writeln!(out, "Backup ID\tOperation\tCreated (UTC)");
        for (id, operation, created) in rows {
            let _ = writeln!(out, "{id}\t{operation}\t{created}");
        }
        return 0;
    }

    let is_file = |path: &str| std::path::Path::new(path).is_file();
    let config_path = match project_config::select(&root, env.config_path.as_deref(), &is_file) {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            let _ = writeln!(err, "Error: {}", project_config::missing_message(&path));
            return 1;
        }
    };
    let config = match config_path.as_deref().map(std::fs::read) {
        Some(Ok(bytes)) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Some(Err(e)) => {
            return fail(
                err,
                Error::io(config_path.as_deref().unwrap_or_default(), e),
            );
        }
        None => None,
    };
    let retention = match backup::configure(
        config_path.as_deref().zip(config.as_deref()),
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
    ) {
        Ok(retention) => retention,
        Err(e) => return fail(err, e),
    };

    let snapshot = match &backup_id {
        Some(id) if *id != paths::leaf(id) => {
            let _ = writeln!(err, "Error: Invalid backup ID: {id}");
            return 1;
        }
        Some(id) => match backup::snapshot_path(&root, id) {
            Ok(snapshot) => snapshot,
            Err(e) => return fail(err, e),
        },
        None => match backup::latest(&root) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                let _ = writeln!(err, "Error: No complete AgentSync backup found");
                return 1;
            }
            Err(e) => return fail(err, e),
        },
    };
    let id = paths::leaf(&snapshot);
    let targets = match backup::load_targets(&root, &snapshot) {
        Ok(targets) => targets,
        Err(e) => return fail(err, e),
    };

    let is_latest = matches!(backup::latest(&root), Ok(Some(latest)) if paths::leaf(&latest) == id);
    let (mut sealed, mut conflict) = (false, None);
    if !force || dry_run {
        match witness::preflight(&root, &snapshot) {
            Preflight::Clean => sealed = true,
            Preflight::Conflict(path) => conflict = Some(path),
            Preflight::Unsealed(detail) => {
                let _ = writeln!(
                    err,
                    "Warning: Backup {id} {detail}; changes made after that operation cannot be detected."
                );
            }
        }
    }
    if let Some(path) = &conflict
        && !dry_run
    {
        report_conflict(err, &id, path, is_latest);
        return 1;
    }

    let _ = writeln!(out, "Rollback plan:");
    let _ = writeln!(out, "  Backup: {id}");
    for target in &targets {
        let action = if target.present { "restore" } else { "remove" };
        let _ = writeln!(out, "  {action:<7} {}", target.rel);
    }
    if dry_run {
        match &conflict {
            Some(path) if force => {
                let _ = writeln!(
                    err,
                    "Warning: Rollback conflict: {path} changed after the operation recorded in backup {id}; --force will overwrite it."
                );
            }
            Some(path) => report_conflict(err, &id, path, is_latest),
            None => {}
        }
        let _ = writeln!(out, "Dry run — nothing was written.");
        return if conflict.is_some() && !force { 1 } else { 0 };
    }
    if !assume_yes && !confirm(&format!("Restore backup {id}?")) {
        let _ = writeln!(out, "Cancelled.");
        return 130;
    }

    let current: Vec<String> = targets
        .iter()
        .map(|target| format!("{root}/{}", target.rel))
        .collect();
    let store = format!("{root}/.ai/backups");
    let pointer = std::path::Path::new(&store).join(".latest");
    let previous_latest = if pointer.is_file() && !pointer.is_symlink() {
        std::fs::read(&pointer)
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let safety = match backup::create(&root, "rollback", &current, retention) {
        Ok(safety) => safety,
        Err(e) => {
            fail(err, e);
            let _ = writeln!(
                err,
                "Error: Could not create a pre-rollback safety backup; no files were changed"
            );
            return 1;
        }
    };

    if sealed && let Preflight::Conflict(path) = witness::preflight(&root, &snapshot) {
        if backup::discard_safety(&store, &safety, &previous_latest).is_err() {
            let _ = writeln!(
                err,
                "Warning: Could not remove the unused safety backup {}.",
                paths::leaf(&safety)
            );
        }
        report_conflict(err, &id, &path, is_latest);
        return 1;
    }

    let mut interrupt = Interrupt::arm();
    let restored = backup::restore(&root, &snapshot);
    let signal = interrupt.received();
    if restored.is_err() || signal.is_some() {
        let status = match (restored, signal) {
            (_, Some(sig)) => interrupt::status(sig),
            (Err(e), None) => fail(err, e),
            (Ok(()), None) => 0,
        };
        let shown = safety.strip_prefix(&format!("{root}/")).unwrap_or(&safety);
        let _ = writeln!(
            err,
            "Warning: Rollback failed; restoring the state from before rollback..."
        );
        match backup::restore(&root, &safety) {
            Ok(()) => {
                if let Err(reason) = witness::seal(&root, &safety) {
                    let _ = writeln!(
                        err,
                        "Warning: Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.",
                        paths::leaf(&safety)
                    );
                }
                let _ = writeln!(err, "Restored pre-rollback state from {shown}");
            }
            Err(e) => {
                fail(err, e);
                let _ = writeln!(
                    err,
                    "Error: Recovery failed. Safety backup retained at {shown}"
                );
            }
        }
        if let Some(sig) = signal {
            interrupt.resend(sig);
        }
        return status;
    }
    drop(interrupt);

    if let Err(reason) = witness::seal(&root, &safety) {
        let _ = writeln!(
            err,
            "Warning: Could not record the post-rollback state ({reason}); rolling back backup {} cannot detect later changes.",
            paths::leaf(&safety)
        );
    }
    if let Err(e) = backup::prune(
        &root,
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
        retention,
    ) {
        fail(err, e);
        let _ = writeln!(err, "Warning: Could not prune old AgentSync backups.");
    }
    let _ = writeln!(out, "Restored backup {id}.");
    let _ = writeln!(out, "Undo backup: {}", paths::leaf(&safety));
    0
}

/// `_rollback_report_conflict`.
fn report_conflict(err: &mut dyn Write, id: &str, path: &str, is_latest: bool) {
    let _ = writeln!(
        err,
        "Error: Rollback conflict: {path} changed after the operation recorded in backup {id}; no files were changed."
    );
    let _ = if is_latest {
        writeln!(
            err,
            "Re-run with --force to restore the backup anyway and discard that change."
        )
    } else {
        writeln!(
            err,
            "Newer AgentSync operations may have changed this target. Roll back the newer backups first, or re-run with --force to restore anyway."
        )
    };
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    struct Output {
        status: u8,
        out: String,
        err: String,
    }

    fn rollback(root: &str, args: &[&str], answer: bool) -> Output {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(
            root,
            &args,
            &Env::default(),
            &mut |_| answer,
            &mut out,
            &mut err,
        );
        Output {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::write(dir.path().join("CLAUDE.md"), "before\n").unwrap();
        (dir, root)
    }

    #[test]
    fn a_rollback_restores_the_latest_backup_and_leaves_an_undo_backup() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md"), format!("{root}/.claude/rules")];
        let snapshot = backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "after\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/rules")).unwrap();
        crate::witness::seal(&root, &snapshot).unwrap();

        let plan = rollback(&root, &["--dry-run"], false);
        assert_eq!(
            plan.out,
            format!(
                "Rollback plan:\n  Backup: {id}\n  restore CLAUDE.md\n  remove  .claude/rules\nDry run — nothing was written.\n"
            )
        );
        let cancelled = rollback(&root, &[], false);
        assert_eq!(cancelled.status, 130);
        assert!(cancelled.out.ends_with("Cancelled.\n"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "after\n"
        );

        let done = rollback(&root, &["--yes"], false);
        assert_eq!(done.status, 0);
        assert_eq!(done.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        assert!(!dir.path().join(".claude/rules").exists());
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(done.out.ends_with(&format!(
            "Restored backup {id}.\nUndo backup: {}\n",
            paths::leaf(&undo)
        )));
        assert_eq!(
            std::fs::read_to_string(format!("{undo}/metadata"))
                .unwrap()
                .lines()
                .nth(1),
            Some("operation=rollback")
        );

        let listed = rollback(&root, &["--list"], false);
        assert!(
            listed
                .out
                .starts_with("Backup ID\tOperation\tCreated (UTC)\n")
        );
        assert!(listed.out.contains(&format!("{id}\tsync\t")));
    }

    #[test]
    fn a_changed_target_refuses_and_force_restores_it() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md")];
        let snapshot = backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "synced\n").unwrap();
        crate::witness::seal(&root, &snapshot).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "edited\n").unwrap();

        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.out, "");
        assert_eq!(
            refused.err,
            format!(
                "Error: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; no files were changed.\nRe-run with --force to restore the backup anyway and discard that change.\n"
            )
        );
        let dry = rollback(&root, &["--dry-run", "--force"], true);
        assert_eq!(dry.status, 0);
        assert_eq!(
            dry.err,
            format!(
                "Warning: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; --force will overwrite it.\n"
            )
        );

        let forced = rollback(&root, &["--force", "--yes"], true);
        assert_eq!(forced.status, 0);
        assert_eq!(forced.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(std::path::Path::new(&format!("{undo}/after.tsv")).is_file());

        std::fs::remove_file(format!("{undo}/after.tsv")).unwrap();
        let unsealed = rollback(&root, &["--yes"], true);
        assert_eq!(unsealed.status, 0);
        assert!(unsealed.err.starts_with(&format!(
            "Warning: Backup {} has no post-operation record; changes made after that operation cannot be detected.\n",
            paths::leaf(&undo)
        )));
    }

    #[test]
    fn arguments_and_ids_are_checked_before_anything_is_read() {
        let (_dir, root) = project();
        let unknown = rollback(&root, &["--nope"], true);
        assert_eq!(unknown.status, 1);
        assert!(
            unknown
                .err
                .starts_with("Error: Unknown rollback option: --nope\nUsage: agentsync rollback")
        );
        assert_eq!(
            rollback(&root, &["a", "b"], true).err,
            "Error: Unexpected rollback argument: b\n"
        );
        assert_eq!(
            rollback(&root, &["../x", "--yes"], true).err,
            "Error: Invalid backup ID: ../x\n"
        );
        assert_eq!(
            rollback(&root, &["x", "--list"], true).err,
            "Error: A backup ID cannot be combined with --list\n"
        );
        assert_eq!(
            rollback(&root, &["--list"], true).out,
            "No AgentSync backups found.\n"
        );
        assert_eq!(
            rollback(&root, &["--yes"], true).err,
            "Error: No complete AgentSync backup found\n"
        );
        assert_eq!(rollback(&root, &["--help", "--nope"], true).out, USAGE);
    }

    #[test]
    fn the_policy_is_checked_after_list_and_preserve_keeps_the_history() {
        let (dir, root) = project();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: typo\n",
        )
        .unwrap();
        let targets = [format!("{root}/CLAUDE.md")];
        backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        assert_eq!(rollback(&root, &["--list"], true).status, 0);
        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(
            refused.err,
            format!(
                "Error: Invalid backup.retention 'typo' in {root}/.ai/agent_sync.yaml; expected bounded or preserve\n"
            )
        );

        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: preserve\n",
        )
        .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            backup_limit: Some("1".into()),
            ..Env::default()
        };
        let args = ["--yes".to_string()];
        assert_eq!(
            run(&root, &args, &env, &mut |_| true, &mut out, &mut err),
            0
        );
        assert_eq!(backup::list(&root).unwrap().len(), 2);

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            config_path: Some("missing.yaml".into()),
            ..Env::default()
        };
        assert_eq!(
            run(&root, &args, &env, &mut |_| true, &mut out, &mut err),
            1
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!(
                "Error: AGENTSYNC_CONFIG_PATH is set but file not found: {root}/missing.yaml\n"
            )
        );
    }
}
