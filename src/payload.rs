//! Where a tool's settings, mcp, or hooks override lives, mirroring the lookups
//! in `lib/helpers/tool_resolver.sh` that `list` reports on.

use std::path::PathBuf;

use crate::{Error, project::Project, tool::Tool};

/// `.ai/src/tools/<slug>/<resource>.*`, first by name: the layout since 0.11.
pub fn find_new_override(
    project: &Project,
    slug: &str,
    resource: &str,
) -> Result<Option<PathBuf>, Error> {
    let dir = project.user_tools_dir().join(slug);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(Error::io(dir, e)),
    };
    let prefix = format!("{resource}.");
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let is_match = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix));
        if is_match && entry.path().is_file() {
            matches.push(entry.path());
        }
    }
    matches.sort();
    Ok(matches.into_iter().next())
}

/// `.ai/src/<resource>/<slug>.<ext>` with the shipped payload's extension: the
/// pre-0.11 flat layout. `None` when no shipped payload fixes an extension.
pub fn legacy_override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf> {
    let ext = tool
        .base_payload(resource)?
        .path()
        .extension()?
        .to_str()?
        .to_string();
    Some(
        project
            .root
            .join(".ai")
            .join("src")
            .join(resource)
            .join(format!("{}.{ext}", tool.slug)),
    )
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
    fn the_first_file_named_after_the_resource_wins() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/cursor/hooks.json.bak", "{}");
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        write(dir.path(), ".ai/src/tools/cursor/mcp.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        let found = find_new_override(&project, "cursor", "hooks").unwrap();
        assert_eq!(
            found,
            Some(dir.path().join(".ai/src/tools/cursor/hooks.json"))
        );
        assert_eq!(
            find_new_override(&project, "cursor", "settings").unwrap(),
            None
        );
        assert_eq!(find_new_override(&project, "zed", "hooks").unwrap(), None);
    }

    #[test]
    fn the_legacy_path_takes_the_shipped_payload_extension() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            legacy_override_path(&project, &claude, "settings"),
            Some(dir.path().join(".ai/src/settings/claude.json"))
        );
        assert_eq!(legacy_override_path(&project, &claude, "hooks"), None);
        let codex = Tool::load(&project, "codex").unwrap();
        assert_eq!(
            legacy_override_path(&project, &codex, "settings"),
            Some(dir.path().join(".ai/src/settings/codex.toml"))
        );
    }
}
