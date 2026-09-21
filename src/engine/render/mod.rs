//! The run of `lib/sync.sh`: config and sources, the `shared:` and engine skill
//! overlays, every enabled tool and selected profile rendered into the
//! workspace, disabled tools cleaned. `check` renders forced and in memory;
//! `cli::sync` runs the stages on disk with its transaction between them.

mod env;
mod passes;
mod prepare;
mod steps;
mod tools;

pub use env::{BackupBounds, Env, Run, Selection};
pub use passes::run_passes;
pub use prepare::{
    banner, check_version_pin, prepare, refuse_configless_cleanup, refuse_escaping_source_links,
    setup_overlays,
};
pub use tools::build_catalog;

use crate::Error;
use crate::engine::session::Session;

pub const TARGET_KEYS: [&str; 9] = [
    "agents",
    "rules",
    "skills",
    "commands",
    "subagents",
    "settings",
    "mcp",
    "hooks",
    "guard",
];

/// A render stopped the way `sync.sh` exits: the status after its log lines.
#[derive(Debug, PartialEq, Eq)]
pub struct Stop(pub u8);

pub type Step = Result<(), Stop>;

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

/// What `check` needs: `sync.sh --force` without its transaction and without
/// `shared:`, which `lib/check.sh` merged into the workspace beforehand.
pub fn render(s: &mut Session, env: &Env) -> Step {
    let mut run = prepare(s, env, Selection::default())?;
    refuse_configless_cleanup(s, &run)?;
    refuse_escaping_source_links(s, &run)?;
    check_version_pin(s, &run)?;
    banner(s);
    setup_overlays(s, &mut run, false)?;
    build_catalog(s, &mut run);
    run_passes(s, &mut run)
}

/// Where a trapped signal ends the run: Bash's trap fires once the command in
/// progress returns.
pub fn checkpoint(s: &Session) -> Step {
    match s.interrupted() {
        Some(status) => Err(Stop(status)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod test_support {
    use crate::engine::session::{Session, test_session};
    use crate::engine::workspace::Content;

    pub(super) fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    pub(super) fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    pub(super) fn project() -> Session {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agents\n");
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: Review\n---\nBody\n",
        );
        s
    }
}
