//! Source overlays of `lib/helpers/shared.sh`: the engine-owned skill layer,
//! per-profile overlays, and the `shared:` parent files `lib/check.sh` merges
//! into its workspace. Overlay trees live under the virtual overlay root.

use std::path::{Path, PathBuf};

use crate::paths::{self, ENGINE_ROOT, OVERLAY_ROOT};
use crate::session::Session;
use crate::workspace::{Content, Workspace};
use crate::{Error, profiles, yaml_subset};

const CATEGORIES: [&str; 4] = ["rules", "skills", "commands", "agents"];

/// The `SOURCE_*` paths a render reads from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub agents: String,
    pub rules: String,
    pub skills: String,
    pub commands: String,
    pub subagents: String,
}

/// `build_overlay_tree`: mirror the child's `AGENTS.md` and category trees,
/// then fill each category with parent files the child lacks. Returns the
/// overlay directory; its `src/` holds the tree.
pub fn build_tree(
    ws: &mut Workspace,
    name: &str,
    child_src: &str,
    parent_src: &str,
    categories: &[&str],
) -> Result<String, Error> {
    let dir = format!("{OVERLAY_ROOT}/{name}");
    ws.remove(&dir);
    let src = format!("{dir}/src");
    ws.create_dir_all(&src);

    if ws.is_dir(child_src) {
        let agents = format!("{child_src}/AGENTS.md");
        if ws.is_file(&agents) {
            ws.copy(&agents, &format!("{src}/AGENTS.md"))?;
        }
        for item in CATEGORIES {
            let from = format!("{child_src}/{item}");
            if ws.is_dir(&from) {
                ws.copy(&from, &format!("{src}/{item}"))?;
            }
        }
    }

    for category in categories {
        let parent_dir = format!("{parent_src}/{category}");
        if !ws.is_dir(&parent_dir) {
            continue;
        }
        for file in ws.files_under(&parent_dir) {
            let rel = &file[parent_dir.len() + 1..];
            let target = format!("{src}/{category}/{rel}");
            if ws.exists(&target) {
                continue;
            }
            ws.create_dir_all(&paths::parent(&target));
            ws.copy(&file, &target)?;
        }
    }
    Ok(dir)
}

/// `_overlay_rewrite_sources`: only the paths the overlay materialised.
pub fn rewrite_sources(ws: &Workspace, dir: &str, sources: &mut Sources) {
    let src = format!("{dir}/src");
    if ws.is_file(&format!("{src}/AGENTS.md")) {
        sources.agents = format!("{src}/AGENTS.md");
    }
    for (category, slot) in [
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("agents", &mut sources.subagents),
    ] {
        let path = format!("{src}/{category}");
        if ws.is_dir(&path) {
            *slot = path;
        }
    }
}

/// `base_src_setup_overlay`: engine-owned skills fill paths the project lacks,
/// unless `base_skills: false`.
pub fn setup_base_src(
    s: &mut Session,
    config: Option<&str>,
    sources: &mut Sources,
) -> Result<(), Error> {
    let base_src = format!("{ENGINE_ROOT}/lib/templates/base-src");
    if !s.ws.is_dir(&format!("{base_src}/skills")) {
        return Ok(());
    }
    if config.is_some_and(|text| yaml_subset::value(text, "base_skills") == "false") {
        return Ok(());
    }
    let child_src = format!("{}/.ai/src", s.paths.root);
    if !s.ws.is_dir(&child_src) {
        return Ok(());
    }
    let dir = build_tree(&mut s.ws, "base-src", &child_src, &base_src, &["skills"])?;
    rewrite_sources(&s.ws, &dir, sources);
    Ok(())
}

/// `profile_setup_overlay`: false when the profile has no `src/` of its own.
pub fn setup_profile(
    s: &mut Session,
    config: &str,
    name: &str,
    base_src: &str,
    sources: &mut Sources,
) -> Result<bool, Error> {
    let overlay = profiles::overlay_dir(config, name);
    let overlay_root = if overlay.starts_with('/') {
        overlay
    } else {
        format!("{}/{overlay}", s.paths.root)
    };
    let profile_src = format!("{overlay_root}/src");
    if !s.ws.is_dir(&profile_src) {
        return Ok(false);
    }
    let dir = build_tree(&mut s.ws, "profile", &profile_src, base_src, &CATEGORIES)?;
    rewrite_sources(&s.ws, &dir, sources);
    s.log
        .info(&format!("Profile overlay active: {name} ({profile_src})"));
    Ok(true)
}

pub fn cleanup_profile(ws: &mut Workspace) {
    ws.remove(&format!("{OVERLAY_ROOT}/profile"));
}

/// `shared_parent_src`: the parent's `.ai/src/`, resolved on disk from the root.
pub fn shared_parent_src(config: &str, root: &str) -> Option<String> {
    let raw = yaml_subset::value(config, "shared.path");
    if raw.is_empty() {
        return None;
    }
    let parent_root = if raw.starts_with('/') {
        raw
    } else {
        format!("{root}/{raw}")
    };
    if !Path::new(&parent_root).is_dir() {
        return None;
    }
    let parent_root = paths::normalize(&parent_root);
    let nested = format!("{parent_root}/.ai/src");
    let parent_src = if Path::new(&nested).is_dir() {
        nested
    } else if paths::leaf(&parent_root) == "src" {
        parent_root
    } else {
        return None;
    };
    (parent_src != format!("{root}/.ai/src")).then_some(parent_src)
}

/// `shared_inherit_categories`: the inherit tokens sync materialises.
pub fn inherit_categories(raw: &str) -> Vec<&'static str> {
    raw.split([',', ' ', '\t', '\n'])
        .filter_map(|token| match token {
            "subagents" | "agents" => Some("agents"),
            "rules" => Some("rules"),
            "skills" => Some("skills"),
            "commands" => Some("commands"),
            _ => None,
        })
        .collect()
}

/// The `shared:` block of `lib/check.sh`: parent files of the inherited
/// categories are merged into the workspace's `.ai/src/` where the project
/// has no file at that path, read from disk as `find -type f` lists them.
pub fn merge_shared_parent(ws: &mut Workspace, parent_src: &str, categories: &[&str]) {
    let child_src = format!("{}/.ai/src", ws.root());
    ws.create_dir_all(&child_src);
    for category in categories {
        let parent_dir = PathBuf::from(format!("{parent_src}/{category}"));
        if !parent_dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_regular_files(&parent_dir, "", &mut files);
        for (rel, disk) in files {
            let target = format!("{child_src}/{category}/{rel}");
            if !ws.exists(&target) {
                ws.insert_file(&target, Content::Disk(disk));
            }
        }
    }
}

fn collect_regular_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            collect_regular_files(&entry.path(), &child_rel, out);
        } else if meta.is_file() {
            out.push((child_rel, entry.path()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;

    fn file(ws: &mut Workspace, path: &str, text: &str) {
        ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    #[test]
    fn the_child_wins_and_the_parent_fills_only_listed_categories() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "child");
        file(&mut s.ws, "/proj/.ai/src/skills/a/SKILL.md", "child a");
        file(&mut s.ws, "/proj/parent/skills/a/SKILL.md", "parent a");
        file(&mut s.ws, "/proj/parent/skills/a/extra.md", "parent extra");
        file(&mut s.ws, "/proj/parent/rules/p.md", "parent rule");
        let dir = build_tree(&mut s.ws, "t", "/proj/.ai/src", "/proj/parent", &["skills"]).unwrap();
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/SKILL.md")).unwrap(),
            b"child a"
        );
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/extra.md")).unwrap(),
            b"parent extra"
        );
        assert!(!s.ws.exists(&format!("{dir}/src/rules")));

        let mut sources = Sources {
            rules: "/proj/.ai/src/rules".into(),
            ..Sources::default()
        };
        rewrite_sources(&s.ws, &dir, &mut sources);
        assert_eq!(sources.agents, "/<agentsync-overlay>/t/src/AGENTS.md");
        assert_eq!(sources.skills, "/<agentsync-overlay>/t/src/skills");
        assert_eq!(sources.rules, "/proj/.ai/src/rules");
    }

    #[test]
    fn the_base_skill_layer_is_skipped_by_base_skills_false() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "a");
        let mut sources = Sources::default();
        setup_base_src(&mut s, Some("base_skills: false\n"), &mut sources).unwrap();
        assert_eq!(sources, Sources::default());
        setup_base_src(&mut s, None, &mut sources).unwrap();
        assert_eq!(sources.skills, "/<agentsync-overlay>/base-src/src/skills");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/agentsync/SKILL.md"));
    }

    #[test]
    fn a_profile_without_src_leaves_sources_alone() {
        let mut s = test_session();
        let config = "profiles:\n  hub:\n    tools: [claude-hub]\n";
        let mut sources = Sources::default();
        assert!(!setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        file(&mut s.ws, "/proj/.ai/profiles/hub/src/rules/hub.md", "h");
        assert!(setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        assert_eq!(sources.rules, "/<agentsync-overlay>/profile/src/rules");
        assert_eq!(
            s.log.tail(1),
            ["[INFO] Profile overlay active: hub (/proj/.ai/profiles/hub/src)"]
        );
    }

    #[test]
    fn inherit_tokens_are_validated_like_shared_setup_overlay() {
        assert_eq!(
            inherit_categories("rules, tools,subagents"),
            ["rules", "agents"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_parent_resolves_on_disk_and_never_to_the_project_itself() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("child");
        std::fs::create_dir_all(root.join(".ai/src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".ai/src/rules")).unwrap();
        std::fs::write(dir.path().join(".ai/src/rules/p.md"), "p").unwrap();
        let root = root.to_string_lossy().into_owned();
        let parent = shared_parent_src("shared:\n  path: \"../\"\n", &root).unwrap();
        assert_eq!(parent, format!("{}/.ai/src", dir.path().to_string_lossy()));
        assert_eq!(shared_parent_src("shared:\n  path: \".\"\n", &root), None);

        let mut ws = Workspace::new(&root);
        merge_shared_parent(&mut ws, &parent, &["rules"]);
        assert_eq!(
            ws.read(&format!("{root}/.ai/src/rules/p.md")).unwrap(),
            b"p"
        );
    }
}
