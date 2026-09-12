//! Layered tool config: user override → shipped base → `base:` variant,
//! resolved per field exactly as `get_tool_value_r` in `lib/helpers/tool_resolver.sh`.

use include_dir::File;

use crate::{Error, catalog, project::Project, yaml_subset};

pub struct Tool {
    pub slug: String,
    user_yaml: Option<String>,
    base_yaml: Option<&'static str>,
}

impl Tool {
    pub fn load(project: &Project, slug: &str) -> Result<Self, Error> {
        let path = project.user_tool_file(slug);
        let user_yaml = if path.is_file() {
            Some(std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?)
        } else {
            None
        };
        Ok(Self::from_parts(
            slug,
            user_yaml,
            catalog::base_tool_yaml(slug),
        ))
    }

    fn from_parts(slug: &str, user_yaml: Option<String>, base_yaml: Option<&'static str>) -> Self {
        Self {
            slug: slug.to_string(),
            user_yaml,
            base_yaml,
        }
    }

    /// `base:` from the user file: the slug a profile variant inherits from.
    pub fn base_name(&self) -> String {
        self.user_yaml
            .as_deref()
            .map(|text| yaml_subset::value(text, "base"))
            .unwrap_or_default()
    }

    /// Effective scalar for a dotted key. A non-empty user value wins; a shipped
    /// base answers next, even with an empty value; only a slug without a
    /// shipped file falls back to its `base:` tool, and never for `base` or `name`.
    pub fn value(&self, key_path: &str) -> String {
        if let Some(user) = &self.user_yaml {
            let found = yaml_subset::value(user, key_path);
            if !found.is_empty() {
                return found;
            }
        }
        if let Some(base) = self.base_yaml {
            return yaml_subset::value(base, key_path);
        }
        if key_path != "base" && key_path != "name" {
            let base_tool = self.base_name();
            if let Some(text) = catalog::base_tool_yaml(&base_tool) {
                return yaml_subset::value(text, key_path);
            }
        }
        String::new()
    }

    pub fn display_name(&self) -> String {
        let name = self.value("name");
        if name.is_empty() {
            self.slug.clone()
        } else {
            name
        }
    }

    /// Shipped payload template for this tool, or for its `base:` tool.
    pub fn base_payload(&self, resource: &str) -> Option<&'static File<'static>> {
        catalog::base_payload(resource, &self.slug).or_else(|| {
            let base_tool = self.base_name();
            if base_tool.is_empty() {
                None
            } else {
                catalog::base_payload(resource, &base_tool)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_with(user: &str) -> Tool {
        Tool::from_parts(
            "claude",
            Some(user.to_string()),
            catalog::base_tool_yaml("claude"),
        )
    }

    #[test]
    fn a_non_empty_user_value_wins() {
        assert_eq!(claude_with("name: \"Mine\"\n").display_name(), "Mine");
    }

    #[test]
    fn an_empty_user_value_falls_back_to_the_base() {
        assert_eq!(claude_with("name:\n").display_name(), "Claude Code");
    }

    #[test]
    fn a_shipped_base_answers_even_when_empty_and_blocks_the_variant_fallback() {
        let tool = claude_with("base: cursor\n");
        assert_eq!(tool.value("targets.rules.extension"), "");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
    }

    #[test]
    fn a_variant_inherits_from_its_base_tool_but_keeps_its_own_identity() {
        let tool = Tool::from_parts(
            "claude-hub",
            Some("base: claude\nprofile_home: \".claude-hub\"\n".to_string()),
            None,
        );
        assert_eq!(tool.base_name(), "claude");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
        assert_eq!(tool.display_name(), "claude-hub");
        let payload = tool
            .base_payload("settings")
            .expect("inherits claude's settings");
        assert!(payload.path().ends_with("claude.json"));
    }

    #[test]
    fn an_unknown_tool_reads_as_empty_and_shows_its_slug() {
        let tool = Tool::from_parts("nope", None, None);
        assert_eq!(tool.value("targets.rules.dest"), "");
        assert_eq!(tool.display_name(), "nope");
        assert!(tool.base_payload("mcp").is_none());
    }
}
