//! AgentSync engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.
//!
//! Module docs name the Bash function each file was ported from. That engine
//! last shipped in 0.37.0: `git show 0.37.0:lib/helpers/<file>.sh`.

pub mod backup;
pub mod catalog;
pub mod changelog;
pub mod cli;
pub mod convert;
pub mod edit_paths;
pub mod error;
pub mod file_ops;
pub mod filters;
pub mod format_rev;
pub mod gitignore;
pub mod interrupt;
pub mod log;
pub mod manifest;
pub mod opencode_json;
pub mod overlay;
pub mod paths;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod project_config;
pub mod prompts;
pub mod render;
pub mod rules;
pub mod session;
pub mod snapshot;
pub mod staging;
pub mod style;
pub mod template_manifest;
pub mod text;
pub mod tool;
pub mod version;
pub mod witness;
pub mod workspace;
pub mod yaml_edit;
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, which `release` bumps with `Cargo.toml`.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_crate_version_carries_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), super::engine_version());
    }
}
