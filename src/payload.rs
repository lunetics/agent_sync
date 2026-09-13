//! Where a tool's settings, mcp, or hooks override lives, mirroring the lookups
//! in `lib/helpers/tool_resolver.sh` that `list` reports on.

use std::path::PathBuf;

use crate::paths::{self, ENGINE_ROOT};
use crate::session::Session;
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

/// `resolve_payload_source`: per-tool override → declared `targets.<res>.source`
/// → legacy flat layout → shared `.ai/src/mcp.json` (mcp only) → shipped base.
pub fn resolve_source(s: &mut Session, tool: &Tool, resource: &str) -> Option<String> {
    let root = s.paths.root.clone();
    let override_dir = format!("{root}/.ai/src/tools/{}", tool.slug);
    if s.ws.is_dir(&override_dir) {
        let prefix = format!("{resource}.");
        let found =
            s.ws.glob(&override_dir)
                .into_iter()
                .map(|name| format!("{override_dir}/{name}"))
                .find(|path| paths::leaf(path).starts_with(&prefix) && s.ws.is_file(path));
        if found.is_some() {
            return found;
        }
    }

    let declared = tool.value(&format!("targets.{resource}.source"));
    if !declared.is_empty() {
        let declared_abs = if declared.starts_with('/') {
            declared.clone()
        } else {
            format!("{root}/{declared}")
        };
        if s.ws.is_file(&declared_abs) {
            if ["hooks", "mcp", "settings"]
                .iter()
                .any(|kind| declared.starts_with(&format!(".ai/src/{kind}/")))
            {
                s.warn_legacy_payload(&declared_abs);
            }
            return Some(declared_abs);
        }
    }

    let base = tool.base_payload(resource);
    if let Some(ext) = base
        .and_then(|file| file.path().extension())
        .and_then(|ext| ext.to_str())
    {
        let legacy = format!("{root}/.ai/src/{resource}/{}.{ext}", tool.slug);
        if s.ws.is_file(&legacy) {
            s.warn_legacy_payload(&legacy);
            return Some(legacy);
        }
    }

    if resource == "mcp" {
        let shared = format!("{root}/.ai/src/mcp.json");
        if s.ws.is_file(&shared) {
            return Some(shared);
        }
    }

    base.and_then(|file| file.path().file_name())
        .and_then(|name| name.to_str())
        .map(|name| format!("{ENGINE_ROOT}/lib/templates/{resource}/{name}"))
}

/// `describe_payload_source`.
pub fn describe_source(root: &str, path: &str, slug: &str, resource: &str) -> &'static str {
    if path.is_empty() {
        return "";
    }
    if path.starts_with(&format!("{root}/.ai/src/tools/{slug}/")) {
        return "override";
    }
    if resource == "mcp" && path == format!("{root}/.ai/src/mcp.json") {
        return "shared";
    }
    if path.starts_with(&format!("{root}/.ai/src/{resource}/")) {
        return "legacy";
    }
    if path.starts_with(&format!("{ENGINE_ROOT}/lib/templates/{resource}/")) {
        return "base";
    }
    "declared"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    #[test]
    fn payload_resolution_follows_the_bash_order() {
        let mut s = test_session();
        let claude = Tool::new("claude", None);
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/<agentsync>/lib/templates/mcp/claude.json")
        );
        s.ws.insert_file("/proj/.ai/src/mcp.json", Content::Bytes(b"{}".to_vec()));
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp.json")
        );
        s.ws.insert_file(
            "/proj/.ai/src/mcp/claude.json",
            Content::Bytes(b"{}".to_vec()),
        );
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp/claude.json")
        );
        assert_eq!(s.log.lines().len(), 3);
        s.ws.insert_file(
            "/proj/.ai/src/tools/claude/mcp.json",
            Content::Bytes(b"{}".to_vec()),
        );
        let found = resolve_source(&mut s, &claude, "mcp").unwrap();
        assert_eq!(found, "/proj/.ai/src/tools/claude/mcp.json");
        assert_eq!(
            describe_source("/proj", &found, "claude", "mcp"),
            "override"
        );
        assert_eq!(
            describe_source(
                "/proj",
                "/<agentsync>/lib/templates/mcp/claude.json",
                "claude",
                "mcp"
            ),
            "base"
        );
        assert_eq!(resolve_source(&mut s, &claude, "hooks"), None);
    }

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
