//! The pending-resolutions readers of `lib/helpers/snapshot.sh` that `resolve`
//! uses. Saving and diffing the catalog belong to `update`.

use std::path::Path;

const PENDING: &str = ".ai/.pending-resolutions.yaml";

fn after_label(stripped: &str, label: &str) -> String {
    let value = stripped[label.len()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    let value = value.strip_suffix('"').unwrap_or(value);
    value.strip_prefix('"').unwrap_or(value).to_string()
}

/// `snapshot_read_pending_pairs`: `(tool, field)` per complete conflict.
pub fn read_pending_pairs(root: &Path) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(root.join(PENDING)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut pairs = Vec::new();
    let (mut in_conflicts, mut tool, mut field) = (false, String::new(), String::new());
    for line in lines {
        let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if stripped.starts_with("conflicts:") {
            in_conflicts = true;
            continue;
        }
        if !in_conflicts {
            continue;
        }
        if !stripped.is_empty() && line == stripped && !stripped.starts_with('-') {
            break;
        }
        if stripped.starts_with("- tool:") {
            if !tool.is_empty() && !field.is_empty() {
                pairs.push((tool.clone(), field.clone()));
            }
            tool = after_label(stripped, "- tool:");
            field.clear();
        } else if stripped.starts_with("field:") {
            field = after_label(stripped, "field:");
        }
    }
    if !tool.is_empty() && !field.is_empty() {
        pairs.push((tool, field));
    }
    pairs
}

/// `snapshot_clear_pending`.
pub fn clear_pending(root: &Path) {
    let _ = std::fs::remove_file(root.join(PENDING));
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn pending_pairs_are_read_from_the_conflicts_list_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        assert!(read_pending_pairs(dir.path()).is_empty());
        std::fs::write(
            dir.path().join(".ai/.pending-resolutions.yaml"),
            "# c\nschema: 1\nconflicts:\n  - tool: \"cursor\"\n    field: \"targets.rules.dest\"\n    base_before: \"a\"\n  - tool: claude\n    field: name\n\nafter: x\n  - tool: \"zed\"\n    field: \"name\"\n",
        )
        .unwrap();
        assert_eq!(
            read_pending_pairs(dir.path()),
            [
                ("cursor".to_string(), "targets.rules.dest".to_string()),
                ("claude".to_string(), "name".to_string()),
            ]
        );
        clear_pending(dir.path());
        assert!(!dir.path().join(".ai/.pending-resolutions.yaml").exists());
        clear_pending(dir.path());
    }
}
