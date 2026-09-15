//! `lib/helpers/format.sh`: the project format revision, a counter bumped only
//! when a project needs a migration step.

use crate::yaml_subset;

const ENGINE_FORMAT_FILE: &str = include_str!("../FORMAT");

fn revision(text: &str) -> u32 {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return 1;
    }
    text.parse().unwrap_or(1)
}

/// `engine_format`: the first line of `FORMAT`, 1 when it is not a number.
pub fn engine() -> u32 {
    revision(ENGINE_FORMAT_FILE.split('\n').next().unwrap_or_default())
}

/// `project_format`: `format:` without its quotes, 1 when absent or not a number.
pub fn project(config: &str) -> u32 {
    revision(&yaml_subset::value(config, "format").replace('"', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_read_as_format_sh_reads_them() {
        assert_eq!(engine(), 2);
        assert_eq!(project("format: 2\n"), 2);
        assert_eq!(project("format: \"3\"\n"), 3);
        assert_eq!(project("tools:\n  enabled: []\n"), 1);
        assert_eq!(project("format: two\n"), 1);
        assert_eq!(project("format: -2\n"), 1);
    }
}
