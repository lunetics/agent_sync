//! State one render shares across its steps: the workspace, path rules, the
//! log, and the manifest's record of what this run wrote (`manifest.sh`).

use std::collections::BTreeSet;

use crate::log::Log;
use crate::paths::Paths;
use crate::workspace::Workspace;

pub struct Session {
    pub ws: Workspace,
    pub paths: Paths,
    pub log: Log,
    touched: BTreeSet<String>,
    legacy_payload_warned: bool,
}

impl Session {
    pub fn new(ws: Workspace, paths: Paths) -> Self {
        Self {
            ws,
            paths,
            log: Log::default(),
            touched: BTreeSet::new(),
            legacy_payload_warned: false,
        }
    }

    pub fn display(&self, path: &str) -> String {
        self.paths.display(path)
    }

    /// `manifest_record_write`: paths outside the root are ignored silently.
    pub fn record_write(&mut self, abs: &str) {
        if let Some(rel) = self.paths.to_repo_relative(abs) {
            self.touched.insert(rel);
        }
    }

    /// `manifest_record_tree`.
    pub fn record_tree(&mut self, dir: &str) {
        for file in self.ws.files_under(dir) {
            self.record_write(&file);
        }
    }

    /// `manifest_was_touched`.
    pub fn was_touched(&self, abs: &str) -> bool {
        self.paths
            .to_repo_relative(abs)
            .is_some_and(|rel| self.touched.contains(&rel))
    }

    pub fn touched(&self) -> &BTreeSet<String> {
        &self.touched
    }

    /// `_warn_legacy_payload_path`: once per run, on stderr.
    pub fn warn_legacy_payload(&mut self, abs: &str) {
        if self.legacy_payload_warned {
            return;
        }
        self.legacy_payload_warned = true;
        let root_prefix = format!("{}/", self.paths.root);
        let rel = abs.strip_prefix(&root_prefix).unwrap_or(abs).to_string();
        self.log
            .err(format!("⚠  Legacy payload override layout detected: {rel}"));
        self.log
            .err("   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).".into());
        self.log
            .err("   Migrate with: agentsync migrate --legacy".into());
    }
}

#[cfg(test)]
pub(crate) fn test_session() -> Session {
    Session::new(Workspace::new("/proj"), Paths::new("/proj", "/proj", None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_recorded_relative_to_the_root_and_outside_paths_are_ignored() {
        let mut s = test_session();
        s.record_write("/proj/CLAUDE.md");
        s.record_write("/elsewhere/x");
        assert!(s.was_touched("/proj/CLAUDE.md"));
        assert_eq!(s.touched().len(), 1);
    }

    #[test]
    fn the_legacy_warning_prints_once() {
        let mut s = test_session();
        s.warn_legacy_payload("/proj/.ai/src/mcp/claude.json");
        s.warn_legacy_payload("/proj/.ai/src/mcp/cursor.json");
        assert_eq!(s.log.lines().len(), 3);
        assert_eq!(
            s.log.tail(3)[0],
            "⚠  Legacy payload override layout detected: .ai/src/mcp/claude.json"
        );
    }
}
