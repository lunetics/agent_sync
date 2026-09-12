//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod cli;
pub mod error;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
