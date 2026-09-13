//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod file_ops;
pub mod filters;
pub mod log;
pub mod opencode_json;
pub mod overlay;
pub mod paths;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod render;
pub mod rules;
pub mod session;
pub mod style;
pub mod text;
pub mod tool;
pub mod workspace;
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
