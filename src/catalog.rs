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
}
