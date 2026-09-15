//! `agentsync upgrade-config`: `cmd_upgrade_config` of `lib/helpers/init.sh`,
//! which pins `agentsync_version` to the running engine.

use std::io::Write;
use std::path::Path;

use super::customize::put;
use crate::style::Style;
use crate::{Error, staging};

const KEY: &str = "agentsync_version:";

/// The `awk` insertion or the `sed` rewrite, as `cmd_upgrade_config` picks it.
pub fn upgrade_text(text: &str, version: &str) -> (String, bool) {
    let pin = format!("agentsync_version: \"{version}\"");
    if text.split('\n').any(|line| line.starts_with(KEY)) {
        let rewritten: Vec<String> = text
            .split('\n')
            .map(|line| {
                if line.starts_with(KEY) {
                    pin.clone()
                } else {
                    line.to_string()
                }
            })
            .collect();
        return (rewritten.join("\n"), false);
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\x0b' | '\x0c' | '\r');
    let mut out = String::new();
    let mut inserted = false;
    for line in lines {
        let stripped = line.trim_start_matches(is_space);
        if !inserted && !(stripped.is_empty() || stripped.starts_with('#')) {
            out.push_str(&format!("{pin}\n\n"));
            inserted = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        out.push_str(&format!("{pin}\n"));
    }
    (out, true)
}

pub fn run(
    root: &Path,
    version: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let config = [
        root.join(".ai").join("agent_sync.yaml"),
        root.join("agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    let Some(config) = config else {
        put(
            err,
            format!(
                "{}: No agent_sync.yaml found in {}\nRun {} first.\n",
                style.red("Error"),
                root.to_string_lossy(),
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let bytes = std::fs::read(&config).map_err(|e| Error::io(&config, e))?;
    let (text, added) = upgrade_text(&String::from_utf8_lossy(&bytes), version);
    staging::write_beside(&config, text.as_bytes())?;
    let shown = style.dim(&config.to_string_lossy());
    let line = if added {
        format!(
            "{}: agentsync_version: {version} → {shown}\n",
            style.green("Added")
        )
    } else {
        format!(
            "{}: agentsync_version → {version} {shown}\n",
            style.green("Updated")
        )
    };
    put(out, line.as_bytes())?;
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_inserted_after_leading_comments_or_every_line_is_rewritten() {
        let cases: [(&str, &str, bool); 5] = [
            (
                "# AgentSync — Project Configuration\ntools:\n  enabled:\n    - claude\n\n",
                "# AgentSync — Project Configuration\nagentsync_version: \"9.9.9\"\n\ntools:\n  enabled:\n    - claude\n\n",
                true,
            ),
            (
                "# head\n\n# more\nformat: 2\nagentsync_version: \"0.1\"\nagentsync_version: \"0.2\"\n",
                "# head\n\n# more\nformat: 2\nagentsync_version: \"9.9.9\"\nagentsync_version: \"9.9.9\"\n",
                false,
            ),
            (
                "# only comments\n\n",
                "# only comments\n\nagentsync_version: \"9.9.9\"\n",
                true,
            ),
            ("", "agentsync_version: \"9.9.9\"\n", true),
            (
                "agentsync_version: 1",
                "agentsync_version: \"9.9.9\"",
                false,
            ),
        ];
        for (text, expected, added) in cases {
            assert_eq!(
                upgrade_text(text, "9.9.9"),
                (expected.to_string(), added),
                "{text:?}"
            );
        }
    }
}
