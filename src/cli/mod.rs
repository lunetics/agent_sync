pub mod check;
pub mod customize;
pub mod diff;
pub mod enable;
pub mod list;
pub mod rollback;
pub mod show;
pub mod sync;
pub mod workspace;

use std::io::Write;

use clap::{Parser, Subcommand};

use crate::Error;
use crate::project::Project;
use crate::style::Style;

/// `tool_resolver_require_project_user_dir`: prints why and returns status 1.
pub(crate) fn refuse_outside_tools_dir(
    project: &Project,
    style: &Style,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    err.write_all(
        format!(
            "{}: source.tools resolves outside the project: {}\nAgentSync only reads that catalog; edit its tool overrides where they live.\n",
            style.red("Error"),
            project.user_tools_dir().to_string_lossy()
        )
        .as_bytes(),
    )
    .map_err(|e| Error::io("<stderr>", e))?;
    Ok(1)
}

/// Argument surface of the ported commands. `bin/agentsync.sh` delegates only
/// the commands in its `_NATIVE_COMMANDS`, so nothing else reaches this parser.
/// Help and version flags are disabled: the Bash CLI owns `--help`, and
/// `--version` must print `agentsync v<VERSION>`, not clap's format.
#[derive(Debug, Parser)]
#[command(
    name = "agentsync",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the engine version.
    #[command(disable_help_flag = true)]
    Version,
    /// Show available tools and their status.
    #[command(visible_alias = "ls", disable_help_flag = true)]
    List,
    /// Verify generated outputs match what sync would write.
    #[command(disable_help_flag = true)]
    Check,
    /// Distribute `.ai/src` to every enabled tool. `lib/sync.sh` parses its
    /// own options, messages and usage included, so they pass through as text.
    #[command(disable_help_flag = true)]
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Restore targets from a backup; parses its own options like `cmd_rollback`.
    #[command(disable_help_flag = true)]
    Rollback {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
