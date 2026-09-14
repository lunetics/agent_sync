//! `agentsync check`: render what `sync --force` would write and compare every
//! managed output with the project, with the messages and exit codes of
//! `lib/check.sh`. Nothing is copied and nothing is written.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::paths::Paths;
use crate::render::{self, Env};
use crate::session::Session;
use crate::workspace::Workspace;
use crate::{Error, engine_version, overlay, yaml_subset};

const MANIFEST_REL: &str = ".ai/.sync-manifest";

pub fn run(root: &str, env: &Env, out: &mut impl Write, err: &mut impl Write) -> Result<u8, Error> {
    let report = check(root, env)?;
    out.write_all(report.stdout.as_bytes())
        .map_err(|e| Error::io("<stdout>", e))?;
    err.write_all(report.stderr.as_bytes())
        .map_err(|e| Error::io("<stderr>", e))?;
    Ok(report.status)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub stdout: String,
    pub stderr: String,
    pub status: u8,
}

impl Report {
    fn out(&mut self, line: &str) {
        self.stdout.push_str(line);
        self.stdout.push('\n');
    }

    fn err(&mut self, line: &str) {
        self.stderr.push_str(line);
        self.stderr.push('\n');
    }
}

pub fn check(root: &str, env: &Env) -> Result<Report, Error> {
    let mut report = Report::default();
    if let Some(message) = version_pin_mismatch(root)? {
        for line in message {
            report.err(&line);
        }
        report.status = 1;
        return Ok(report);
    }
    report.out("Checking AgentSync configuration synchronization...");

    let manifest = manifest_paths(root)?;
    let ws = match seed_workspace(root, &manifest) {
        Ok(ws) => ws,
        Err(detail) => {
            report.out("❌ Failed to prepare temporary workspace for check");
            report.err(&detail);
            report.status = 1;
            return Ok(report);
        }
    };

    let mut session = Session::new(ws, Paths::for_disk_root(root));
    merge_shared_parent(&mut session.ws, root)?;
    if render::render(&mut session, env).is_err() {
        report.out("❌ Sync script failed during check");
        report.out("Sync output (last 40 lines):");
        for line in session.log.tail(40) {
            report.out(line);
        }
        report.status = 1;
        return Ok(report);
    }

    let mut compare: BTreeSet<String> = manifest.into_iter().collect();
    for rel in session.touched() {
        if session.ws.is_file(&format!("{root}/{rel}")) {
            compare.insert(rel.clone());
        }
    }

    let mut differences = Vec::new();
    for rel in compare {
        let expected = format!("{root}/{rel}");
        let actual = Path::new(root).join(&rel);
        match (session.ws.is_file(&expected), actual.is_file()) {
            (true, true) => {
                let rendered = session.ws.read(&expected)?;
                let on_disk = std::fs::read(&actual).map_err(|e| Error::io(&actual, e))?;
                if rendered != on_disk {
                    differences.push(format!("Files {rel} differ"));
                }
            }
            (true, false) => differences.push(format!("Missing: {rel}")),
            (false, true) => differences.push(format!("No longer generated: {rel}")),
            (false, false) => {}
        }
    }

    if differences.is_empty() {
        report.out("✅ AgentSync configurations are safe and synced.");
        return Ok(report);
    }
    report.out("");
    report.out("⚠️  AgentSync configurations are out of sync with source.");
    report.out("Differences detected (showing up to 20):");
    for line in differences.iter().take(20) {
        report.out(line);
    }
    report.out("");
    report.out("Please run: lib/sync.sh");
    report.status = 1;
    Ok(report)
}

/// The config `lib/check.sh` reads: `.ai/agent_sync.yaml`, else a root-level one.
fn project_config(root: &str) -> Result<Option<String>, Error> {
    for rel in [".ai/agent_sync.yaml", "agent_sync.yaml"] {
        let path = Path::new(root).join(rel);
        if path.is_file() {
            let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
            return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(None)
}

/// `_check_version_pin`: fatal only for committed outputs.
fn version_pin_mismatch(root: &str) -> Result<Option<Vec<String>>, Error> {
    let Some(config) = project_config(root)? else {
        return Ok(None);
    };
    if yaml_subset::value(&config, "outputs").replace('"', "") != "committed" {
        return Ok(None);
    }
    let pinned = yaml_subset::value(&config, "agentsync_version").replace('"', "");
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return Ok(None);
    }
    Ok(Some(vec![
        format!(
            "❌ This project pins agentsync {pinned} but you are running {engine} — committed outputs must come from one version everywhere."
        ),
        format!("  • Match the pin:  agentsync update {pinned}"),
        format!(
            "  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)"
        ),
    ]))
}

/// `manifest_paths`: the text before the first tab of every complete line.
fn manifest_paths(root: &str) -> Result<Vec<String>, Error> {
    let path = Path::new(root).join(MANIFEST_REL);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    Ok(lines
        .into_iter()
        .map(|line| line.trim_start_matches('\t'))
        .map(|line| line.split('\t').next().unwrap_or(""))
        .filter(|rel| !rel.is_empty())
        .map(str::to_string)
        .collect())
}

/// What `lib/check.sh` copied with `tar`: `.ai/` without backups, a root
/// `agent_sync.yaml`, and every manifest output that exists.
fn seed_workspace(root: &str, manifest: &[String]) -> Result<Workspace, String> {
    let mut ws = Workspace::new(root);
    let ai = Path::new(root).join(".ai");
    if !ai.exists() {
        return Err("Incomplete copy — missing: .ai".to_string());
    }
    let skip_in_ai = |rel: &str| rel == "backups" || rel.starts_with("backups/") || is_git(rel);
    ws.seed_from_disk(&format!("{root}/.ai"), &ai, &skip_in_ai)
        .map_err(|e| e.to_string())?;
    let config = Path::new(root).join("agent_sync.yaml");
    if config.is_file() {
        ws.seed_from_disk(&format!("{root}/agent_sync.yaml"), &config, &is_git)
            .map_err(|e| e.to_string())?;
    }
    for rel in manifest {
        if rel.starts_with(".ai/") || is_git(rel) {
            continue;
        }
        let disk = Path::new(root).join(rel);
        if disk.exists() {
            ws.seed_from_disk(&format!("{root}/{rel}"), &disk, &is_git)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(ws)
}

fn is_git(rel: &str) -> bool {
    rel.split('/').any(|segment| segment == ".git")
}

/// The `shared:` block of `lib/check.sh`, before the render.
fn merge_shared_parent(ws: &mut Workspace, root: &str) -> Result<(), Error> {
    let Some(config) = project_config(root)? else {
        return Ok(());
    };
    if let Some(parent) = overlay::shared_parent_src(&config, root) {
        let inherit = yaml_subset::value(&config, "shared.inherit");
        overlay::merge_shared_parent(ws, &parent, &overlay::inherit_categories(&inherit))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        write(&root, ".ai/src/AGENTS.md", "# Agents\n");
        write(
            &root,
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let root = root.to_string_lossy().into_owned();
        (dir, root)
    }

    #[test]
    fn a_project_never_synced_reports_every_output_missing() {
        let (_dir, root) = project();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking AgentSync configuration synchronization...\n\n⚠️  AgentSync configurations are out of sync with source.\nDifferences detected (showing up to 20):\n"));
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
        assert!(report.stdout.ends_with("\nPlease run: lib/sync.sh\n"));
    }

    #[test]
    fn manifest_outputs_that_match_the_render_are_in_sync() {
        let (_dir, root) = project();
        let mut session = Session::new(
            seed_workspace(&root, &[]).unwrap(),
            Paths::for_disk_root(&root),
        );
        render::render(&mut session, &Env::default()).unwrap();
        let mut manifest = String::new();
        for rel in session.touched() {
            let bytes = session.ws.read(&format!("{root}/{rel}")).unwrap();
            write(Path::new(&root), rel, &String::from_utf8(bytes).unwrap());
            manifest.push_str(&format!("{rel}\thash\n"));
        }
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let clean = check(&root, &Env::default()).unwrap();
        assert_eq!(
            clean.stdout,
            "Checking AgentSync configuration synchronization...\n✅ AgentSync configurations are safe and synced.\n"
        );
        assert_eq!(clean.status, 0);

        write(Path::new(&root), "CLAUDE.md", "edited\n");
        write(Path::new(&root), ".cursor/rules/core.mdc", "stale\n");
        manifest.push_str(".cursor/rules/core.mdc\thash\n");
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let dirty = check(&root, &Env::default()).unwrap();
        assert!(dirty.stdout.contains("Files CLAUDE.md differ\n"));
        assert!(
            dirty
                .stdout
                .contains("No longer generated: .cursor/rules/core.mdc\n")
        );
    }

    #[test]
    fn a_committed_pin_mismatch_fails_before_the_banner() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "outputs: committed\nagentsync_version: \"0.0.1\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert_eq!(report.stdout, "");
        assert!(
            report
                .stderr
                .starts_with("❌ This project pins agentsync 0.0.1 but you are running ")
        );
    }

    #[test]
    fn a_failed_render_prints_the_log_tail() {
        let (_dir, root) = project();
        std::fs::remove_file(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking AgentSync configuration synchronization...\n❌ Sync script failed during check\nSync output (last 40 lines):\n[ERROR] Source agents file not found: "));
    }

    #[test]
    fn a_missing_ai_directory_fails_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(
            report,
            Report {
                stdout: "Checking AgentSync configuration synchronization...\n❌ Failed to prepare temporary workspace for check\n".into(),
                stderr: "Incomplete copy — missing: .ai\n".into(),
                status: 1,
            }
        );
    }

    #[test]
    fn manifest_lines_keep_the_text_before_the_first_tab_of_complete_lines() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            MANIFEST_REL,
            "a.md\th\n\tb.md\th\nc.md\nlast\th",
        );
        assert_eq!(manifest_paths(&root).unwrap(), ["a.md", "b.md", "c.md"]);
    }
}
