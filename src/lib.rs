//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod error;
pub mod filters;
pub mod log;
pub mod paths;
pub mod payload;
pub mod project;
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
