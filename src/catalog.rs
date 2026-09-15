//! Templates shipped with the engine, embedded at build time from `lib/templates/`.

use include_dir::{Dir, File, include_dir};

static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/lib/templates");

/// Shipped `lib/templates/tools/<slug>.yaml`, when the slug is a base tool.
pub fn base_tool_yaml(slug: &str) -> Option<&'static str> {
    TEMPLATES
        .get_file(format!("tools/{slug}.yaml"))?
        .contents_utf8()
}

/// Base tool slugs in byte order, `_`-prefixed entries such as `_TEMPLATE` skipped.
pub fn base_tools() -> Vec<String> {
    let mut slugs: Vec<String> = files_in("tools")
        .filter_map(|file| file_name(file)?.strip_suffix(".yaml").map(str::to_string))
        .filter(|stem| !stem.starts_with('_'))
        .collect();
    slugs.sort();
    slugs.dedup();
    slugs
}

/// First shipped `lib/templates/<resource>/<slug>.*` by name, as the Bash glob picks it.
pub fn base_payload(resource: &str, slug: &str) -> Option<&'static File<'static>> {
    let prefix = format!("{slug}.");
    let mut matches: Vec<&'static File<'static>> = files_in(resource)
        .filter(|file| file_name(file).is_some_and(|name| name.starts_with(&prefix)))
        .collect();
    matches.sort_by(|a, b| a.path().cmp(b.path()));
    matches.first().copied()
}

/// `_dedupe_load_template_set`: the shipped `AGENTS.md`, the `*.md` files of
/// `rules`, `commands`, and `agents`, and every file below `skills` whose name
/// does not start with `.`, as paths below `.ai/src/` in byte order.
pub fn template_sources() -> Vec<String> {
    let mut paths: Vec<String> = engine_files()
        .into_iter()
        .filter_map(|(path, _)| {
            let rel = path.strip_prefix("lib/templates/")?;
            let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            let shipped = match dir {
                "" => rel == "AGENTS.md",
                "rules" | "commands" | "agents" => name.ends_with(".md") && !name.starts_with('.'),
                _ => (dir == "skills" || dir.starts_with("skills/")) && !name.starts_with('.'),
            };
            shipped.then(|| rel.to_string())
        })
        .collect();
    paths.sort();
    paths
}

/// `lib/prompts/migrate.md`, the upgrade prompt `agentsync migrate` prints.
pub const MIGRATE_PROMPT: &str = include_str!("../lib/prompts/migrate.md");

/// The engine-owned skills under `lib/templates/base-src/skills/`, in byte order.
pub fn base_src_skills() -> Vec<String> {
    TEMPLATES
        .get_dir("base-src/skills")
        .into_iter()
        .flat_map(|dir| dir.dirs())
        .filter_map(|dir| Some(dir.path().file_name()?.to_str()?.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// `lib/config.yaml`, the install-dir global config `sync.sh` reads source defaults from.
pub const GLOBAL_CONFIG: &str = include_str!("../lib/config.yaml");

/// Every embedded engine file as its `/`-separated path below the engine root.
pub fn engine_files() -> Vec<(String, &'static [u8])> {
    let mut files = vec![("lib/config.yaml".to_string(), GLOBAL_CONFIG.as_bytes())];
    collect_files(&TEMPLATES, &mut files);
    files
}

fn collect_files(dir: &'static Dir<'static>, out: &mut Vec<(String, &'static [u8])>) {
    for file in dir.files() {
        let rel: Vec<String> = file
            .path()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        out.push((format!("lib/templates/{}", rel.join("/")), file.contents()));
    }
    for sub in dir.dirs() {
        collect_files(sub, out);
    }
}

fn files_in(dir: &str) -> impl Iterator<Item = &'static File<'static>> {
    TEMPLATES
        .get_dir(dir)
        .into_iter()
        .flat_map(|found| found.files())
}

fn file_name<'a>(file: &'a File<'_>) -> Option<&'a str> {
    file.path().file_name()?.to_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_lists_the_thirteen_shipped_tools_without_the_template() {
        let slugs = base_tools();
        assert_eq!(slugs.len(), 13);
        assert_eq!(slugs.first().map(String::as_str), Some("amazonq"));
        assert_eq!(slugs.last().map(String::as_str), Some("zed"));
        assert!(!slugs.iter().any(|s| s.starts_with('_')));
    }

    #[test]
    fn a_base_tool_yaml_is_embedded_verbatim() {
        let yaml = base_tool_yaml("claude").expect("claude is shipped");
        assert!(yaml.contains("name: \"Claude Code\""));
        assert!(base_tool_yaml("nope").is_none());
    }

    #[test]
    fn a_base_payload_is_found_by_slug_and_resource() {
        let file = base_payload("settings", "claude").expect("shipped");
        assert_eq!(
            file.path().file_name().and_then(|n| n.to_str()),
            Some("claude.json")
        );
        assert_eq!(base_payload("hooks", "zed").map(|f| f.path()), None);
        assert!(base_payload("hooks", "claude-hub").is_none());
    }

    #[test]
    fn the_engine_owns_the_agentsync_skill_and_ships_the_migrate_prompt() {
        assert_eq!(base_src_skills(), ["agentsync"]);
        assert!(MIGRATE_PROMPT.starts_with("I need you to safely migrate"));
    }

    #[test]
    fn the_template_sources_are_the_set_dedupe_loads() {
        assert_eq!(
            template_sources(),
            [
                "AGENTS.md",
                "agents/code-reviewer.md",
                "commands/fix-issue.md",
                "commands/review.md",
                "rules/comments.md",
                "rules/core.md",
                "rules/git.md",
                "skills/comments/SKILL.md",
                "skills/commit/SKILL.md",
                "skills/debug/SKILL.md",
                "skills/humanizer/SKILL.md",
                "skills/humanizer/references/wikipedia_signs_of_ai_writing.md",
                "skills/humanizer/scripts/strip-ai-chars.sh",
                "skills/prompt-engineering/SKILL.md",
                "skills/prompt-engineering/references/agent-persona.md",
                "skills/prompt-engineering/references/metaprompting.md",
                "skills/prompt-engineering/references/snippets.md",
                "skills/refactor/SKILL.md",
                "skills/review/SKILL.md",
            ]
        );
    }
}
