//! The project being operated on: its root and `agent_sync.yaml`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::{Error, yaml_subset};

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
}

impl Project {
    /// `AGENTSYNC_REPO_ROOT` when set, else the working directory.
    pub fn discover() -> Result<Self, Error> {
        let root = match std::env::var_os("AGENTSYNC_REPO_ROOT") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => std::env::current_dir().map_err(|e| Error::io(".", e))?,
        };
        Self::at(root)
    }

    /// Config is `.ai/agent_sync.yaml`, falling back to a root-level `agent_sync.yaml`.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::ProjectRootNotFound(root));
        }
        let config_path = [".ai/agent_sync.yaml", "agent_sync.yaml"]
            .into_iter()
            .map(|rel| root.join(rel))
            .find(|path| path.is_file());
        Ok(Self { root, config_path })
    }

    pub fn user_tools_dir(&self) -> PathBuf {
        self.root.join(".ai").join("src").join("tools")
    }

    pub fn user_tool_file(&self, slug: &str) -> PathBuf {
        self.user_tools_dir().join(format!("{slug}.yaml"))
    }

    pub fn shared_mcp_path(&self) -> PathBuf {
        self.root.join(".ai").join("src").join("mcp.json")
    }

    /// Slugs with a `.ai/src/tools/<slug>.yaml`, in byte order, `_`-prefixed skipped.
    pub fn user_override_tools(&self) -> Result<Vec<String>, Error> {
        let dir = self.user_tools_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(dir, e)),
        };
        let mut slugs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".yaml")) else {
                continue;
            };
            if stem.starts_with('_') || !entry.path().is_file() {
                continue;
            }
            slugs.push(stem.to_string());
        }
        slugs.sort();
        slugs.dedup();
        Ok(slugs)
    }

    fn config_text(&self) -> Result<Option<String>, Error> {
        match &self.config_path {
            None => Ok(None),
            Some(path) => std::fs::read_to_string(path)
                .map(Some)
                .map_err(|e| Error::io(path, e)),
        }
    }

    /// `tools.enabled` from the project config.
    pub fn configured_enabled_tools(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .config_text()?
            .map(|text| yaml_subset::list(&text, "tools.enabled"))
            .unwrap_or_default())
    }

    /// Override files that still carry the pre-`tools.enabled` `enabled: true`.
    pub fn legacy_enabled_tools(&self) -> Result<Vec<String>, Error> {
        let mut enabled = Vec::new();
        for slug in self.user_override_tools()? {
            let path = self.user_tool_file(&slug);
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            if yaml_subset::value(&text, "enabled") == "true" {
                enabled.push(slug);
            }
        }
        Ok(enabled)
    }

    /// Union of the configured and legacy enabled sets.
    pub fn enabled_tools(&self) -> Result<BTreeSet<String>, Error> {
        let mut set: BTreeSet<String> = self.configured_enabled_tools()?.into_iter().collect();
        set.extend(self.legacy_enabled_tools()?);
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_dot_ai_config_wins_over_a_root_level_one() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "agent_sync.yaml", "tools:\n  enabled: [zed]\n");
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(
            project.config_path,
            Some(dir.path().join(".ai/agent_sync.yaml"))
        );
        assert_eq!(project.configured_enabled_tools().unwrap(), ["claude"]);
    }

    #[test]
    fn a_project_without_config_or_overrides_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.config_path, None);
        assert!(project.user_override_tools().unwrap().is_empty());
        assert!(project.enabled_tools().unwrap().is_empty());
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = Project::at(dir.path().join("nope")).unwrap_err();
        assert!(matches!(err, Error::ProjectRootNotFound(_)));
    }

    #[test]
    fn override_tools_skip_the_template_and_non_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/zed.yaml", "name: Z\n");
        write(dir.path(), ".ai/src/tools/claude.yaml", "name: C\n");
        write(dir.path(), ".ai/src/tools/_TEMPLATE.yaml", "name: T\n");
        write(dir.path(), ".ai/src/tools/notes.md", "");
        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.user_override_tools().unwrap(), ["claude", "zed"]);
    }

    #[test]
    fn enabled_tools_union_the_config_list_and_legacy_flags() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled:\n    - claude\n    - zed\n",
        );
        write(dir.path(), ".ai/src/tools/zed.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/cursor.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/kimi.yaml", "enabled: false\n");
        let project = Project::at(dir.path()).unwrap();
        let set = project.enabled_tools().unwrap();
        let enabled: Vec<&str> = set.iter().map(String::as_str).collect();
        assert_eq!(enabled, ["claude", "cursor", "zed"]);
    }
}
