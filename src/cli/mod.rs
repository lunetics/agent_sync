pub mod check;
pub mod list;
pub mod sync;
pub mod workspace;

use clap::{Parser, Subcommand};

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
}
