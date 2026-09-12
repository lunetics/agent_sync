# Rust Migration Phase 1: Native Engine Foundation and `list`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Land a Rust crate at the repository root, a Bash dispatcher that hands ported commands to its binary, and `version` and `list` as the first natively served commands, with no change for anyone who has no binary.

**Architecture:** Strangler. `bin/agentsync.sh` stays the entry point and delegates a command listed in `_NATIVE_COMMANDS` to `target/release/agentsync` when `AGENTSYNC_NATIVE` allows; every other command stays Bash. The test seam is the CLI process boundary every bats file already uses: `run_agentsync` with `AGENTSYNC_NATIVE=1` exercises the binary, `tests/native_parity.bats` diffs both engines on one fixture, and `cargo test` covers the pure modules. The config reader ports the semantics of `lib/helpers/yaml.sh` instead of parsing YAML, so every existing config reads identically.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2; dev: assert_cmd 2, predicates 3, tempfile 3. Bash 3.2 for the dispatcher, bats-core for conformance. Design: `docs/specs/2026-09-12-rust-migration-design.md`.

## Global Constraints

- `.ai/src/` remains the source of truth; nothing in this phase writes under a user project.
- No binary ships to users in this phase: without a built binary every command runs in Bash exactly as in 0.35.2.
- `bin/agentsync.sh` stays Bash 3.2-compatible and clean under `shellcheck -x -S warning -e SC1091`.
- A ported command matches Bash byte for byte on stdout, stderr, and exit status when stdout is not a terminal; on a terminal it emits the escape codes of `lib/helpers/cli_colors.sh`.
- Rust: `unsafe_code = "forbid"`; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean; no YAML crate, `yaml_subset` mirrors `lib/helpers/yaml.sh`.
- `VERSION` is the only version source; `Cargo.toml` keeps `0.0.0` until Phase 5.
- Accepted deviations, recorded in the design spec: clap rejects stray arguments to a ported command with exit 2 where Bash ignored them; tool slugs sort in byte order where Bash used locale `sort`.
- Commits follow Conventional Commits, scope `native` for engine work, imperative subject, no attribution trailers.

---

### Task 0: Toolchain, Branch, Baseline

**Files:**
- None changed.

**Interfaces:**
- Consumes: a clean `main` at or after `432b2dd`.
- Produces: branch `feat/native-engine-phase-1`, a working `cargo`, and a recorded green baseline.

- [x] **Step 1: Install the Rust toolchain (one-time, on the developer machine)**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default
. "$HOME/.cargo/env"
cargo --version && rustc --version
```

Expected: `cargo 1.8x` or newer and a matching `rustc`; both at least 1.85.

- [x] **Step 2: Branch off `main`**

```bash
git -C /Users/yelamanyelmuratov/Development/agent_sync/agent status --short
git -C /Users/yelamanyelmuratov/Development/agent_sync/agent switch -c feat/native-engine-phase-1
```

Expected: empty status; `Switched to a new branch 'feat/native-engine-phase-1'`.

- [x] **Step 3: Record the baseline**

```bash
bats --jobs 4 tests/ --tap | tail -3
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
```

Expected: the TAP plan is `1..725` with no `not ok`; ShellCheck exits 0.

---

### Task 0b: Prefactor — `list` Survives an Override Without `enabled: true`

**Files:**
- Modify: `lib/helpers/tool_resolver.sh:481-493` (`list_legacy_enabled_tools`)
- Modify: `tests/list.bats` (one regression test)

**Interfaces:**
- Consumes: nothing new.
- Produces: `list_legacy_enabled_tools` always returns 0, so `list_enabled_tools` inside `$(...)` under `set -e` can no longer abort its caller. Found while writing Task 7's fixtures: with `.ai/src/tools/cursor.yaml` holding only `name:`, `agentsync list` printed its header and exited 1 without a message, because the function's status was that of the last `[[ "$flag" == "true" ]]`. `doctor.sh:428` already works around it with `|| true`; `list.sh:71` does not. The parity suite needs a correct Bash reference, so the fix lands before the port.

- [x] **Step 1: Write the failing test in `tests/list.bats`**

```bash
@test "list survives a tool override that does not set enabled" {
    mkdir -p .ai/src/tools
    printf 'name: "My Cursor"\n' > .ai/src/tools/cursor.yaml
    run run_agentsync list
    [ "$status" -eq 0 ]
    [[ "$output" == *"My Cursor"* ]]
    [[ "$output" == *"1 tool override(s)"* ]]
}
```

- [x] **Step 2: Run it, confirm it fails**

Run: `bats tests/list.bats`
Expected: the new test fails with `status` 1; the other 7 pass.

- [x] **Step 3: Return 0 from the lister**

In `lib/helpers/tool_resolver.sh`, `list_legacy_enabled_tools`, after the `done` that closes the `for f in "$dir"/*.yaml` loop, add `return 0`:

```bash
        flag=$(parse_yaml_value "$f" "enabled")
        [[ "$flag" == "true" ]] && echo "$base"
    done
    return 0
}
```

- [x] **Step 4: Run the affected files, confirm green**

```bash
bats tests/list.bats tests/doctor.bats tests/enable.bats
shellcheck -x -S warning -e SC1091 lib/helpers/tool_resolver.sh
```

Expected: 8 + 34 + 13 tests pass; ShellCheck exits 0.

- [x] **Step 5: Commit**

```bash
git add lib/helpers/tool_resolver.sh tests/list.bats
git commit -m "fix(list): survive a tool override without enabled: true"
```

---

### Task 1: Crate Scaffold and the `version` Command

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/cli/mod.rs`
- Create: `src/main.rs`
- Create: `tests/cli.rs`
- Modify: `.gitignore` (add `/target/`)
- Modify: `.github/workflows/ci.yaml` (add the `native` job)

**Interfaces:**
- Consumes: the `VERSION` file.
- Produces: `agentsync::engine_version() -> &'static str`; `agentsync::Error` with `Error::io(path, source)`, `Error::ProjectRootNotFound(PathBuf)`, `Error::StaleBinary { binary, engine }`, `Error::is_broken_pipe()`; `agentsync::cli::{Cli, Command}`; the binary `target/release/agentsync` printing `agentsync v<VERSION>` for `version`, `--version`, `-v`, and refusing to run when `AGENTSYNC_ENGINE_VERSION` names another version.

- [x] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "agentsync"
# The VERSION file is the release source of truth (auto-tag, `agentsync release`);
# this field stays 0.0.0 until Phase 5 wires cargo-dist to it.
version = "0.0.0"
edition = "2024"
rust-version = "1.85"
description = "Sync AI agent instructions from .ai/src/ to every tool."
license = "GPL-3.0-only"
repository = "https://github.com/yelmuratoff/agent"
publish = false

[dependencies]
clap = { version = "4.6", features = ["derive"] }
include_dir = "0.7"
thiserror = "2"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"

[lints.rust]
unsafe_code = "forbid"

[profile.release]
codegen-units = 1
lto = true
strip = true
```

- [x] **Step 2: Write the failing integration test `tests/cli.rs`**

```rust
use assert_cmd::Command;
use predicates::prelude::*;

fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

fn agentsync() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agentsync"))
}

#[test]
fn version_prints_the_engine_version() {
    agentsync()
        .arg("version")
        .assert()
        .success()
        .stdout(format!("agentsync v{}\n", engine_version()));
}

#[test]
fn version_flags_match_the_bash_cli() {
    for flag in ["--version", "-v"] {
        agentsync()
            .arg(flag)
            .assert()
            .success()
            .stdout(format!("agentsync v{}\n", engine_version()));
    }
}

#[test]
fn a_stale_binary_refuses_to_run() {
    agentsync()
        .env("AGENTSYNC_ENGINE_VERSION", "0.0.1")
        .arg("version")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cargo build --release"));
}

#[test]
fn a_matching_engine_version_is_accepted() {
    agentsync()
        .env("AGENTSYNC_ENGINE_VERSION", engine_version())
        .arg("version")
        .assert()
        .success();
}
```

- [x] **Step 3: Run it, confirm it fails**

Run: `cargo test`
Expected: compilation error, `src/main.rs` and `src/lib.rs` do not exist.

- [x] **Step 4: Write `src/error.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Repository root not found: {}", .0.display())]
    ProjectRootNotFound(PathBuf),
    #[error("native binary is v{binary} but the engine is v{engine}. Rebuild it: cargo build --release")]
    StaleBinary { binary: String, engine: String },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn is_broken_pipe(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == std::io::ErrorKind::BrokenPipe)
    }
}
```

- [x] **Step 5: Write `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod cli;
pub mod error;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
```

- [x] **Step 6: Write `src/cli/mod.rs`**

```rust
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
}
```

- [x] **Step 7: Write `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use agentsync::cli::{Cli, Command};
use agentsync::{Error, engine_version};
use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), Error> {
    guard_engine_version()?;
    // The Bash CLI accepts these flags as commands; clap's own version flag is off.
    if matches!(args.first().and_then(|a| a.to_str()), Some("--version" | "-v")) {
        return print_version();
    }
    let cli = Cli::parse_from(std::iter::once(OsString::from("agentsync")).chain(args));
    match cli.command {
        Command::Version => print_version(),
    }
}

fn print_version() -> Result<(), Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version()).map_err(|e| Error::io("<stdout>", e))
}

/// `bin/agentsync.sh` passes its own VERSION so a binary left behind by an
/// older checkout can never answer for a newer engine.
fn guard_engine_version() -> Result<(), Error> {
    let Some(engine) = std::env::var_os("AGENTSYNC_ENGINE_VERSION") else {
        return Ok(());
    };
    let engine = engine.to_string_lossy().into_owned();
    if engine.is_empty() || engine == engine_version() {
        return Ok(());
    }
    Err(Error::StaleBinary {
        binary: engine_version().to_string(),
        engine,
    })
}
```

- [x] **Step 8: Run the tests and the lints, confirm green**

```bash
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: 4 tests pass; both lints exit 0.

- [x] **Step 9: Ignore the build directory**

Append to `.gitignore`, after the `# Temporary agent task bundles` block:

```gitignore
# Rust build output
/target/
```

- [x] **Step 10: Add the `native` CI job**

Append to `.github/workflows/ci.yaml` under `jobs:`:

```yaml
  native:
    name: Native engine (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 20
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - run: cargo build --release
```

- [x] **Step 11: Commit**

```bash
git add Cargo.toml Cargo.lock src/ tests/cli.rs .gitignore .github/workflows/ci.yaml
git commit -m "feat(native): scaffold the Rust engine with a version command"
```

---

### Task 2: Dispatcher in `bin/agentsync.sh`

**Files:**
- Modify: `bin/agentsync.sh` (new functions before `# ─── Main`, one call inside `main`)
- Modify: `tests/test_helper.bash` (default `AGENTSYNC_NATIVE=0`)
- Create: `tests/native_dispatch.bats`

**Interfaces:**
- Consumes: the binary contract from Task 1 (`AGENTSYNC_ENGINE_VERSION` guard).
- Produces: `_NATIVE_COMMANDS` (space-padded list), `_native_bin` (prints the binary path or returns 1), `_native_try "$@"` (exits with the binary's status or returns 1 to fall through); the env contract `AGENTSYNC_NATIVE` ∈ {`0`, `1`, unset} and `AGENTSYNC_NATIVE_BIN`.

- [x] **Step 1: Write the failing tests `tests/native_dispatch.bats`**

```bash
#!/usr/bin/env bats
# Tests for the Bash → native engine delegation in bin/agentsync.sh.

load test_helper

setup() {
    setup_test_project
    FAKE_BIN="$TEST_PROJECT/fake-agentsync"
    cat > "$FAKE_BIN" <<'EOF'
#!/usr/bin/env bash
printf 'native:%s\n' "$1"
printf 'arg:%s\n' "$@"
printf 'engine:%s\n' "${AGENTSYNC_ENGINE_VERSION:-unset}"
exit 42
EOF
    chmod +x "$FAKE_BIN"
    export AGENTSYNC_NATIVE_BIN="$FAKE_BIN"
}

teardown() { teardown_test_project; }

@test "native: a ported command runs the binary and returns its exit status" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version
    [ "$status" -eq 42 ]
    [[ "$output" == *"native:version"* ]]
}

@test "native: the engine version travels with the call" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version
    [[ "$output" == *"engine:$(cat "$REPO_ROOT/VERSION")"* ]]
}

@test "native: arguments reach the binary untouched" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version --flag "two words"
    [[ "$output" == *$'arg:--flag\narg:two words'* ]]
}

@test "native: AGENTSYNC_NATIVE=0 keeps a ported command in Bash" {
    export AGENTSYNC_NATIVE=0
    run run_agentsync version
    [ "$status" -eq 0 ]
    [[ "$output" == agentsync\ v* ]]
}

@test "native: an unported command never reaches the binary" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync help
    [ "$status" -eq 0 ]
    [[ "$output" == *"COMMANDS"* ]]
    [[ "$output" != *"native:"* ]]
}

@test "native: without AGENTSYNC_NATIVE a missing binary falls back to Bash" {
    unset AGENTSYNC_NATIVE
    export AGENTSYNC_NATIVE_BIN="$TEST_PROJECT/missing"
    run run_agentsync version
    [ "$status" -eq 0 ]
    [[ "$output" == agentsync\ v* ]]
}

@test "native: AGENTSYNC_NATIVE=1 without a binary fails loudly" {
    export AGENTSYNC_NATIVE=1
    export AGENTSYNC_NATIVE_BIN="$TEST_PROJECT/missing"
    run run_agentsync version
    [ "$status" -eq 1 ]
    [[ "$output" == *"no native binary"* ]]
}
```

- [x] **Step 2: Run them, confirm they fail**

Run: `bats tests/native_dispatch.bats`
Expected: the first three and the last test fail (`version` is answered by Bash, status 0); the other three pass by accident, which is fine.

- [x] **Step 3: Default the suite to Bash in `tests/test_helper.bash`**

Insert after the `unset AGENTSYNC_ALLOW_POST_SYNC ...` line:

```bash
# Ported commands run in Bash unless a run opts into the native engine
# (`AGENTSYNC_NATIVE=1 bats tests/`), so a stray release build never changes
# what the suite exercises.
export AGENTSYNC_NATIVE="${AGENTSYNC_NATIVE:-0}"
```

- [x] **Step 4: Add the delegation to `bin/agentsync.sh`**

Insert before `# ─── Main ───`:

```bash
# ─── Native engine delegation ───────────────────────────────────────────────
# Public env contract: AGENTSYNC_NATIVE is "0" to force Bash, "1" to require
# the native binary and fail loudly without one, unset to use one when found;
# AGENTSYNC_NATIVE_BIN names the binary explicitly.
_NATIVE_COMMANDS=" version --version -v "

_native_bin() {
    if [[ -n "${AGENTSYNC_NATIVE_BIN:-}" ]]; then
        [[ -x "$AGENTSYNC_NATIVE_BIN" ]] || return 1
        echo "$AGENTSYNC_NATIVE_BIN"
        return 0
    fi
    local candidate
    for candidate in \
        "$_AGENTSYNC_ENGINE_ROOT/target/release/agentsync" \
        "$_AGENTSYNC_ENGINE_ROOT/target/release/agentsync.exe" \
        "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync-native" \
        "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync-native.exe"; do
        if [[ -x "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

# Delegate the whole argument list to the native binary when the command is
# ported and a binary is available. Exits with the binary's status; returns 1
# to fall through to the Bash implementation.
_native_try() {
    local command="${1:-help}"
    local mode="${AGENTSYNC_NATIVE:-}"
    [[ "$mode" != "0" ]] || return 1
    [[ "$_NATIVE_COMMANDS" == *" $command "* ]] || return 1

    local bin
    if ! bin=$(_native_bin); then
        if [[ "$mode" == "1" ]]; then
            echo "$(_red "Error"): AGENTSYNC_NATIVE=1 but no native binary was found." >&2
            echo "  Build one with: cargo build --release" >&2
            exit 1
        fi
        return 1
    fi

    # A child process, not exec: the EXIT trap must still remove the run tmpdir.
    local rc=0
    AGENTSYNC_ENGINE_VERSION="$VERSION" "$bin" "$@" || rc=$?
    exit "$rc"
}
```

Inside `main`, insert one line after the `--help` interception `case` block and before `case "$command" in` / `init)`:

```bash
    # `|| true`: falling through to Bash is a return 1, which errexit would
    # otherwise treat as a failed command and abort the run.
    _native_try "$@" || true
```

`|| true` is load-bearing, not defensive: `_native_try` signals "stay in Bash"
with `return 1`, and a bare call of it under `set -euo pipefail` aborts `main`
before the dispatch `case` — every command, ported or not, exits 1 with no
output. Amended after `bats tests/cli.bats` went 0/8 on the first write.

Delegation sits after `check_for_updates` and the `--help` interception on purpose: the update banner, the format notice, and `<cmd> --help` keep their Bash behaviour for every command.

- [x] **Step 5: Run the tests, confirm green**

```bash
bats tests/native_dispatch.bats tests/cli.bats
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: 7 + 8 tests pass; ShellCheck exits 0.

- [x] **Step 6: Prove the real binary goes through the dispatcher**

```bash
cargo build --release
AGENTSYNC_NATIVE=1 bats tests/cli.bats
AGENTSYNC_NATIVE=1 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh version
```

Expected: 8 tests pass; the last command prints `agentsync v0.35.2` (or the current `VERSION`).

- [x] **Step 7: Commit**

```bash
git add bin/agentsync.sh tests/test_helper.bash tests/native_dispatch.bats
git commit -m "feat(cli): delegate ported commands to the native engine"
```

---

### Task 3: `yaml_subset` Reader

**Files:**
- Create: `src/yaml_subset.rs`
- Modify: `src/lib.rs` (add `pub mod yaml_subset;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `yaml_subset::value(text: &str, key_path: &str) -> String` (empty when missing or empty, first match wins), `yaml_subset::list(text: &str, key_path: &str) -> Vec<String>` (inline `[a, b]` or block `- item`), `yaml_subset::normalize_scalar(raw: &str) -> String`. Semantics are those of `parse_yaml_value_r`, `parse_yaml_list`, and `_yaml_normalize_scalar_reply` in `lib/helpers/yaml.sh`.

- [x] **Step 1: Write the module with its failing tests**

```rust
//! Reader for the YAML subset AgentSync configs are written in.
//!
//! This mirrors `lib/helpers/yaml.sh` rather than parsing YAML: every shipped
//! and user config was written against that reader's rules (first duplicate
//! key wins, `#` ends an unquoted value, `\n` stays literal inside quotes),
//! and the migration promises byte-identical outputs.

fn strip_indent(line: &str) -> (usize, &str) {
    let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
    (line.len() - stripped.len(), stripped)
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `(key, rest)` when the stripped line starts with a bare key and a colon.
fn split_key(stripped: &str) -> Option<(&str, &str)> {
    let (key, rest) = stripped.split_once(':')?;
    if key.is_empty() || !key.chars().all(is_key_char) {
        return None;
    }
    Some((key, rest.trim_start_matches(|c: char| c.is_ascii_whitespace())))
}

/// Only a fully empty line or a comment line is skipped; a whitespace-only
/// line still counts for indentation, as in Bash.
fn is_blank_or_comment(line: &str) -> bool {
    line.is_empty() || strip_indent(line).1.starts_with('#')
}

/// Trim, unwrap one layer of matching quotes (verbatim inside, no escape
/// processing), otherwise cut at the first `#`.
pub fn normalize_scalar(raw: &str) -> String {
    let value = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    for quote in ['"', '\''] {
        if let Some(inner) = unwrap_quoted(value, quote) {
            return inner.to_string();
        }
    }
    let unquoted = value.split('#').next().unwrap_or("");
    unquoted
        .trim_end_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

/// `"inner"` optionally followed by whitespace and a `# comment`. The closing
/// quote is the last one that leaves only such a tail, which is what Bash's
/// greedy `^"(.*)"[[:space:]]*(#.*)?$` picks.
fn unwrap_quoted(value: &str, quote: char) -> Option<&str> {
    let body = value.strip_prefix(quote)?;
    body.rmatch_indices(quote).find_map(|(idx, _)| {
        let tail = body[idx + quote.len_utf8()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        (tail.is_empty() || tail.starts_with('#')).then_some(&body[..idx])
    })
}

/// The scalar at a dotted key path (`targets.rules.dest`), or `""` when the
/// key is missing or empty. Nesting is decided by indentation and the first
/// occurrence of a key wins.
pub fn value(text: &str, key_path: &str) -> String {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut level = 0usize;
    let mut section_indent = 0usize;
    let mut in_section = false;

    for line in text.lines() {
        if is_blank_or_comment(line) {
            continue;
        }
        let (indent, stripped) = strip_indent(line);
        let Some((key, rest)) = split_key(stripped) else {
            continue;
        };
        if !in_section {
            if indent != 0 || key != keys[0] {
                continue;
            }
        } else {
            if indent <= section_indent {
                return String::new();
            }
            if key != keys[level] {
                continue;
            }
        }
        if level + 1 == keys.len() {
            return normalize_scalar(rest);
        }
        in_section = true;
        section_indent = indent;
        level += 1;
    }
    String::new()
}

/// Items of the list at a dotted key path: a single-line `[a, b]` or a block
/// of `- item` lines. Empty when the key is missing.
pub fn list(text: &str, key_path: &str) -> Vec<String> {
    let inline = value(text, key_path);
    if let Some(items) = inline.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return items
            .split(',')
            .map(normalize_scalar)
            .filter(|item| !item.is_empty())
            .collect();
    }
    block_list(text, key_path)
}

fn block_list(text: &str, key_path: &str) -> Vec<String> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut items = Vec::new();
    let mut level = 0usize;
    let mut section_indent = 0usize;
    let mut in_section = false;
    let mut collecting = false;
    let mut list_indent: Option<usize> = None;

    for line in text.lines() {
        if is_blank_or_comment(line) {
            continue;
        }
        let (indent, stripped) = strip_indent(line);

        if collecting {
            if let Some(item) = stripped.strip_prefix('-') {
                let expected = *list_indent.get_or_insert(indent);
                if indent == expected {
                    let item = normalize_scalar(item);
                    if !item.is_empty() {
                        items.push(item);
                    }
                    continue;
                }
            }
            if list_indent.is_some_and(|expected| indent < expected) {
                return items;
            }
            continue;
        }

        let Some((key, _)) = split_key(stripped) else {
            continue;
        };
        if !in_section {
            if indent != 0 || key != keys[0] {
                continue;
            }
        } else {
            if indent <= section_indent {
                return items;
            }
            if key != keys[level] {
                continue;
            }
        }
        if level + 1 == keys.len() {
            collecting = true;
            continue;
        }
        in_section = true;
        section_indent = indent;
        level += 1;
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# A config in the shapes the Bash reader accepts.
name: "Claude Code"
enabled: false # switched off
count: 3
empty:
targets:
  rules:
    dest: ".claude/rules"
    header: "---\nglobs: '**/*'\n---"
  skills:
    dest: '.claude/skills' # single quotes
tools:
  enabled:
    - claude
    - "cursor"
    - codex   # trailing comment
inline: [a, b , "c"]
dup: first
dup: second
url: http://example.com/x#frag
"#;

    #[test]
    fn reads_a_quoted_root_scalar() {
        assert_eq!(value(SAMPLE, "name"), "Claude Code");
    }

    #[test]
    fn drops_an_inline_comment_from_an_unquoted_scalar() {
        assert_eq!(value(SAMPLE, "enabled"), "false");
        assert_eq!(value(SAMPLE, "count"), "3");
    }

    #[test]
    fn keeps_escapes_literal_inside_quotes() {
        assert_eq!(value(SAMPLE, "targets.rules.header"), r"---\nglobs: '**/*'\n---");
    }

    #[test]
    fn walks_nested_keys_by_indent() {
        assert_eq!(value(SAMPLE, "targets.rules.dest"), ".claude/rules");
        assert_eq!(value(SAMPLE, "targets.skills.dest"), ".claude/skills");
    }

    #[test]
    fn a_missing_nested_key_does_not_leak_into_the_next_section() {
        assert_eq!(value(SAMPLE, "targets.rules.missing"), "");
    }

    #[test]
    fn missing_empty_and_section_keys_all_read_as_empty() {
        assert_eq!(value(SAMPLE, "missing"), "");
        assert_eq!(value(SAMPLE, "empty"), "");
        assert_eq!(value(SAMPLE, "targets"), "");
    }

    #[test]
    fn the_first_duplicate_key_wins() {
        assert_eq!(value(SAMPLE, "dup"), "first");
    }

    #[test]
    fn an_unquoted_value_ends_at_the_first_hash() {
        assert_eq!(value(SAMPLE, "url"), "http://example.com/x");
    }

    #[test]
    fn a_quoted_value_keeps_its_hash_and_drops_a_trailing_comment() {
        assert_eq!(value("k: \"a # b\" # c\n", "k"), "a # b");
    }

    #[test]
    fn a_key_without_a_space_after_the_colon_still_parses() {
        assert_eq!(value("k:v\n", "k"), "v");
    }

    #[test]
    fn reads_a_block_list() {
        assert_eq!(list(SAMPLE, "tools.enabled"), ["claude", "cursor", "codex"]);
    }

    #[test]
    fn reads_an_inline_list() {
        assert_eq!(list(SAMPLE, "inline"), ["a", "b", "c"]);
    }

    #[test]
    fn a_missing_key_yields_no_list() {
        assert!(list(SAMPLE, "missing").is_empty());
        assert!(list("k: v\n", "k").is_empty());
    }

    #[test]
    fn a_block_list_ends_at_a_shallower_line() {
        let text = "tools:\n  enabled:\n    - claude\n  other: x\n    - not-an-item\n";
        assert_eq!(list(text, "tools.enabled"), ["claude"]);
    }

    // Design spec, "Known quirks", item 1: reproduced on purpose until cutover.
    #[test]
    fn an_empty_block_key_takes_the_next_dash_list_like_bash_does() {
        let text = "tools:\n  enabled:\nother:\n  - stolen\n";
        assert_eq!(list(text, "tools.enabled"), ["stolen"]);
    }
}
```

- [x] **Step 2: Register the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod cli;
pub mod error;
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
```

- [x] **Step 3: Run the tests, confirm green**

Run: `cargo test yaml_subset`
Expected: 15 tests pass. If `an_empty_block_key_takes_the_next_dash_list_like_bash_does` fails, the port drifted from `parse_yaml_list`; re-read `lib/helpers/yaml.sh:160-234` before changing the test.

- [x] **Step 4: Cross-check two values against the Bash reader**

```bash
bash -c 'source lib/helpers/yaml.sh; parse_yaml_value lib/templates/tools/cursor.yaml targets.rules.header; parse_yaml_list lib/templates/tools/cursor.yaml targets.rules.include'
```

Expected: the literal `---\nglobs: '**/*'\nalwaysApply: true\n---` (backslash-n, not newlines) and no list output. These are the same answers `value` and `list` give for that file.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/yaml_subset.rs src/lib.rs
git commit -m "feat(native): read the AgentSync YAML subset"
```

---

### Task 4: Project Config, Embedded Catalog, Layered Tool Values

**Files:**
- Create: `src/project.rs`
- Create: `src/catalog.rs`
- Create: `src/tool.rs`
- Modify: `src/lib.rs` (register the three modules)

**Interfaces:**
- Consumes: `yaml_subset::{value, list}`, `Error`.
- Produces:
  - `Project { root: PathBuf, config_path: Option<PathBuf> }` with `Project::discover()`, `Project::at(root)`, `user_tools_dir()`, `user_tool_file(slug)`, `shared_mcp_path()`, `user_override_tools() -> Result<Vec<String>>`, `configured_enabled_tools()`, `legacy_enabled_tools()`, `enabled_tools() -> Result<BTreeSet<String>>`.
  - `catalog::base_tool_yaml(slug) -> Option<&'static str>`, `catalog::base_tools() -> Vec<String>`, `catalog::base_payload(resource, slug) -> Option<&'static include_dir::File<'static>>`.
  - `Tool { slug }` with `Tool::load(&Project, slug) -> Result<Tool>`, `base_name()`, `value(key_path) -> String`, `display_name()`, `base_payload(resource)`.

- [x] **Step 1: Write `src/catalog.rs`**

```rust
//! Templates shipped with the engine, embedded at build time from `lib/templates/`.

use include_dir::{Dir, File, include_dir};

static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/lib/templates");

/// Shipped `lib/templates/tools/<slug>.yaml`, when the slug is a base tool.
pub fn base_tool_yaml(slug: &str) -> Option<&'static str> {
    TEMPLATES.get_file(format!("tools/{slug}.yaml"))?.contents_utf8()
}

/// Base tool slugs in byte order, `_`-prefixed entries such as `_TEMPLATE` skipped.
pub fn base_tools() -> Vec<String> {
    let mut slugs: Vec<String> = files_in("tools")
        .filter_map(|file| file_name(file)?.strip_suffix(".yaml").map(str::to_string))
        .filter(|stem| !stem.starts_with('_'))
        .collect();
    slugs.sort();
    slugs.dedup();
    slugs
}

/// First shipped `lib/templates/<resource>/<slug>.*` by name, as the Bash glob picks it.
pub fn base_payload(resource: &str, slug: &str) -> Option<&'static File<'static>> {
    let prefix = format!("{slug}.");
    let mut matches: Vec<&'static File<'static>> = files_in(resource)
        .filter(|file| file_name(file).is_some_and(|name| name.starts_with(&prefix)))
        .collect();
    matches.sort_by(|a, b| a.path().cmp(b.path()));
    matches.first().copied()
}

fn files_in(dir: &str) -> impl Iterator<Item = &'static File<'static>> {
    TEMPLATES.get_dir(dir).into_iter().flat_map(|found| found.files())
}

// `path()` borrows from the `File` reference, not from the embedded `'static`
// bytes, so the returned name carries the reference's lifetime.
fn file_name<'a>(file: &'a File<'_>) -> Option<&'a str> {
    file.path().file_name()?.to_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_lists_the_thirteen_shipped_tools_without_the_template() {
        let slugs = base_tools();
        assert_eq!(slugs.len(), 13);
        assert_eq!(slugs.first().map(String::as_str), Some("amazonq"));
        assert_eq!(slugs.last().map(String::as_str), Some("zed"));
        assert!(!slugs.iter().any(|s| s.starts_with('_')));
    }

    #[test]
    fn a_base_tool_yaml_is_embedded_verbatim() {
        let yaml = base_tool_yaml("claude").expect("claude is shipped");
        assert!(yaml.contains("name: \"Claude Code\""));
        assert!(base_tool_yaml("nope").is_none());
    }

    #[test]
    fn a_base_payload_is_found_by_slug_and_resource() {
        let file = base_payload("settings", "claude").expect("shipped");
        assert_eq!(file.path().file_name().and_then(|n| n.to_str()), Some("claude.json"));
        assert_eq!(base_payload("hooks", "zed").map(|f| f.path()), None);
        assert!(base_payload("hooks", "claude-hub").is_none());
    }
}
```

- [x] **Step 2: Write `src/project.rs`**

```rust
//! The project being operated on: its root and `agent_sync.yaml`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::{Error, yaml_subset};

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
}

impl Project {
    /// `AGENTSYNC_REPO_ROOT` when set, else the working directory.
    pub fn discover() -> Result<Self, Error> {
        let root = match std::env::var_os("AGENTSYNC_REPO_ROOT") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => std::env::current_dir().map_err(|e| Error::io(".", e))?,
        };
        Self::at(root)
    }

    /// Config is `.ai/agent_sync.yaml`, falling back to a root-level `agent_sync.yaml`.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::ProjectRootNotFound(root));
        }
        let config_path = [".ai/agent_sync.yaml", "agent_sync.yaml"]
            .into_iter()
            .map(|rel| root.join(rel))
            .find(|path| path.is_file());
        Ok(Self { root, config_path })
    }

    pub fn user_tools_dir(&self) -> PathBuf {
        self.root.join(".ai").join("src").join("tools")
    }

    pub fn user_tool_file(&self, slug: &str) -> PathBuf {
        self.user_tools_dir().join(format!("{slug}.yaml"))
    }

    pub fn shared_mcp_path(&self) -> PathBuf {
        self.root.join(".ai").join("src").join("mcp.json")
    }

    /// Slugs with a `.ai/src/tools/<slug>.yaml`, in byte order, `_`-prefixed skipped.
    pub fn user_override_tools(&self) -> Result<Vec<String>, Error> {
        let dir = self.user_tools_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(dir, e)),
        };
        let mut slugs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".yaml")) else {
                continue;
            };
            if stem.starts_with('_') || !entry.path().is_file() {
                continue;
            }
            slugs.push(stem.to_string());
        }
        slugs.sort();
        slugs.dedup();
        Ok(slugs)
    }

    fn config_text(&self) -> Result<Option<String>, Error> {
        match &self.config_path {
            None => Ok(None),
            Some(path) => std::fs::read_to_string(path)
                .map(Some)
                .map_err(|e| Error::io(path, e)),
        }
    }

    /// `tools.enabled` from the project config.
    pub fn configured_enabled_tools(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .config_text()?
            .map(|text| yaml_subset::list(&text, "tools.enabled"))
            .unwrap_or_default())
    }

    /// Override files that still carry the pre-`tools.enabled` `enabled: true`.
    pub fn legacy_enabled_tools(&self) -> Result<Vec<String>, Error> {
        let mut enabled = Vec::new();
        for slug in self.user_override_tools()? {
            let path = self.user_tool_file(&slug);
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            if yaml_subset::value(&text, "enabled") == "true" {
                enabled.push(slug);
            }
        }
        Ok(enabled)
    }

    /// Union of the configured and legacy enabled sets.
    pub fn enabled_tools(&self) -> Result<BTreeSet<String>, Error> {
        let mut set: BTreeSet<String> = self.configured_enabled_tools()?.into_iter().collect();
        set.extend(self.legacy_enabled_tools()?);
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_dot_ai_config_wins_over_a_root_level_one() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "agent_sync.yaml", "tools:\n  enabled: [zed]\n");
        write(dir.path(), ".ai/agent_sync.yaml", "tools:\n  enabled: [claude]\n");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.config_path, Some(dir.path().join(".ai/agent_sync.yaml")));
        assert_eq!(project.configured_enabled_tools().unwrap(), ["claude"]);
    }

    #[test]
    fn a_project_without_config_or_overrides_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.config_path, None);
        assert!(project.user_override_tools().unwrap().is_empty());
        assert!(project.enabled_tools().unwrap().is_empty());
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = Project::at(dir.path().join("nope")).unwrap_err();
        assert!(matches!(err, Error::ProjectRootNotFound(_)));
    }

    #[test]
    fn override_tools_skip_the_template_and_non_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/zed.yaml", "name: Z\n");
        write(dir.path(), ".ai/src/tools/claude.yaml", "name: C\n");
        write(dir.path(), ".ai/src/tools/_TEMPLATE.yaml", "name: T\n");
        write(dir.path(), ".ai/src/tools/notes.md", "");
        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.user_override_tools().unwrap(), ["claude", "zed"]);
    }

    #[test]
    fn enabled_tools_union_the_config_list_and_legacy_flags() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n    - zed\n");
        write(dir.path(), ".ai/src/tools/zed.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/cursor.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/kimi.yaml", "enabled: false\n");
        let project = Project::at(dir.path()).unwrap();
        let set = project.enabled_tools().unwrap();
        let enabled: Vec<&str> = set.iter().map(String::as_str).collect();
        assert_eq!(enabled, ["claude", "cursor", "zed"]);
    }
}
```

- [x] **Step 3: Write `src/tool.rs`**

```rust
//! Layered tool config: user override → shipped base → `base:` variant,
//! resolved per field exactly as `get_tool_value_r` in `lib/helpers/tool_resolver.sh`.

use include_dir::File;

use crate::{Error, catalog, project::Project, yaml_subset};

pub struct Tool {
    pub slug: String,
    user_yaml: Option<String>,
    base_yaml: Option<&'static str>,
}

impl Tool {
    pub fn load(project: &Project, slug: &str) -> Result<Self, Error> {
        let path = project.user_tool_file(slug);
        let user_yaml = if path.is_file() {
            Some(std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?)
        } else {
            None
        };
        Ok(Self::from_parts(slug, user_yaml, catalog::base_tool_yaml(slug)))
    }

    fn from_parts(slug: &str, user_yaml: Option<String>, base_yaml: Option<&'static str>) -> Self {
        Self {
            slug: slug.to_string(),
            user_yaml,
            base_yaml,
        }
    }

    /// `base:` from the user file: the slug a profile variant inherits from.
    pub fn base_name(&self) -> String {
        self.user_yaml
            .as_deref()
            .map(|text| yaml_subset::value(text, "base"))
            .unwrap_or_default()
    }

    /// Effective scalar for a dotted key. A non-empty user value wins; a shipped
    /// base answers next, even with an empty value; only a slug without a
    /// shipped file falls back to its `base:` tool, and never for `base` or `name`.
    pub fn value(&self, key_path: &str) -> String {
        if let Some(user) = &self.user_yaml {
            let found = yaml_subset::value(user, key_path);
            if !found.is_empty() {
                return found;
            }
        }
        if let Some(base) = self.base_yaml {
            return yaml_subset::value(base, key_path);
        }
        if key_path != "base" && key_path != "name" {
            let base_tool = self.base_name();
            if let Some(text) = catalog::base_tool_yaml(&base_tool) {
                return yaml_subset::value(text, key_path);
            }
        }
        String::new()
    }

    pub fn display_name(&self) -> String {
        let name = self.value("name");
        if name.is_empty() { self.slug.clone() } else { name }
    }

    /// Shipped payload template for this tool, or for its `base:` tool.
    pub fn base_payload(&self, resource: &str) -> Option<&'static File<'static>> {
        catalog::base_payload(resource, &self.slug).or_else(|| {
            let base_tool = self.base_name();
            if base_tool.is_empty() {
                None
            } else {
                catalog::base_payload(resource, &base_tool)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_with(user: &str) -> Tool {
        Tool::from_parts("claude", Some(user.to_string()), catalog::base_tool_yaml("claude"))
    }

    #[test]
    fn a_non_empty_user_value_wins() {
        assert_eq!(claude_with("name: \"Mine\"\n").display_name(), "Mine");
    }

    #[test]
    fn an_empty_user_value_falls_back_to_the_base() {
        assert_eq!(claude_with("name:\n").display_name(), "Claude Code");
    }

    #[test]
    fn a_shipped_base_answers_even_when_empty_and_blocks_the_variant_fallback() {
        let tool = claude_with("base: cursor\n");
        assert_eq!(tool.value("targets.rules.extension"), "");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
    }

    #[test]
    fn a_variant_inherits_from_its_base_tool_but_keeps_its_own_identity() {
        let tool = Tool::from_parts(
            "claude-hub",
            Some("base: claude\nprofile_home: \".claude-hub\"\n".to_string()),
            None,
        );
        assert_eq!(tool.base_name(), "claude");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
        assert_eq!(tool.display_name(), "claude-hub");
        let payload = tool.base_payload("settings").expect("inherits claude's settings");
        assert!(payload.path().ends_with("claude.json"));
    }

    #[test]
    fn an_unknown_tool_reads_as_empty_and_shows_its_slug() {
        let tool = Tool::from_parts("nope", None, None);
        assert_eq!(tool.value("targets.rules.dest"), "");
        assert_eq!(tool.display_name(), "nope");
        assert!(tool.base_payload("mcp").is_none());
    }
}
```

- [x] **Step 4: Register the modules in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod error;
pub mod project;
pub mod tool;
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
```

- [x] **Step 5: Run the tests, confirm green**

Run: `cargo test`
Expected: the 15 `yaml_subset` tests plus 3 `catalog`, 5 `project`, 5 `tool` tests and the 4 integration tests pass.

- [x] **Step 6: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/catalog.rs src/project.rs src/tool.rs src/lib.rs
git commit -m "feat(native): resolve project config and layered tool values"
```

---

### Task 5: Payload Override Discovery

**Files:**
- Create: `src/payload.rs`
- Modify: `src/lib.rs` (add `pub mod payload;`)

**Interfaces:**
- Consumes: `Project`, `Tool::base_payload`, `Error`.
- Produces: `payload::find_new_override(&Project, slug, resource) -> Result<Option<PathBuf>>` (`.ai/src/tools/<slug>/<resource>.*`) and `payload::legacy_override_path(&Project, &Tool, resource) -> Option<PathBuf>` (`.ai/src/<resource>/<slug>.<ext>` with the shipped payload's extension; the caller checks existence).

- [x] **Step 1: Write `src/payload.rs` with its tests**

```rust
//! Where a tool's settings, mcp, or hooks override lives, mirroring the lookups
//! in `lib/helpers/tool_resolver.sh` that `list` reports on.

use std::path::PathBuf;

use crate::{Error, project::Project, tool::Tool};

/// `.ai/src/tools/<slug>/<resource>.*`, first by name: the layout since 0.11.
pub fn find_new_override(project: &Project, slug: &str, resource: &str) -> Result<Option<PathBuf>, Error> {
    let dir = project.user_tools_dir().join(slug);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(e) => return Err(Error::io(dir, e)),
    };
    let prefix = format!("{resource}.");
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let is_match = entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix));
        if is_match && entry.path().is_file() {
            matches.push(entry.path());
        }
    }
    matches.sort();
    Ok(matches.into_iter().next())
}

/// `.ai/src/<resource>/<slug>.<ext>` with the shipped payload's extension: the
/// pre-0.11 flat layout. `None` when no shipped payload fixes an extension.
pub fn legacy_override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf> {
    let ext = tool.base_payload(resource)?.path().extension()?.to_str()?.to_string();
    Some(
        project
            .root
            .join(".ai")
            .join("src")
            .join(resource)
            .join(format!("{}.{ext}", tool.slug)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_file_named_after_the_resource_wins() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/cursor/hooks.json.bak", "{}");
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        write(dir.path(), ".ai/src/tools/cursor/mcp.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        let found = find_new_override(&project, "cursor", "hooks").unwrap();
        assert_eq!(found, Some(dir.path().join(".ai/src/tools/cursor/hooks.json")));
        assert_eq!(find_new_override(&project, "cursor", "settings").unwrap(), None);
        assert_eq!(find_new_override(&project, "zed", "hooks").unwrap(), None);
    }

    #[test]
    fn the_legacy_path_takes_the_shipped_payload_extension() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            legacy_override_path(&project, &claude, "settings"),
            Some(dir.path().join(".ai/src/settings/claude.json"))
        );
        assert_eq!(legacy_override_path(&project, &claude, "hooks"), None);
        let codex = Tool::load(&project, "codex").unwrap();
        assert_eq!(
            legacy_override_path(&project, &codex, "settings"),
            Some(dir.path().join(".ai/src/settings/codex.toml"))
        );
    }
}
```

- [x] **Step 2: Register the module in `src/lib.rs`**

Add `pub mod payload;` between `pub mod error;` and `pub mod project;`.

- [x] **Step 3: Run the tests, confirm green**

Run: `cargo test payload`
Expected: 2 tests pass.

- [x] **Step 4: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/payload.rs src/lib.rs
git commit -m "feat(native): locate payload overrides"
```

---

### Task 6: `list` Served Natively

**Files:**
- Create: `src/style.rs`
- Create: `src/cli/list.rs`
- Modify: `src/cli/mod.rs` (add `List`)
- Modify: `src/main.rs` (dispatch `List`, red `Error` prefix)
- Modify: `src/lib.rs` (add `pub mod style;`)
- Modify: `tests/cli.rs` (list smoke tests)
- Modify: `bin/agentsync.sh` (`_NATIVE_COMMANDS` gains `list ls`)
- Modify: `tests/native_dispatch.bats` (`list --help` stays Bash)

**Interfaces:**
- Consumes: `Project`, `Tool`, `catalog::base_tools`, `payload::*`.
- Produces: `Style::for_stdout()`, `Style::plain()`, `bold/green/cyan/yellow/red/dim(&str) -> String`, `style::pad_right(&str, usize) -> String`; `cli::list::render(&Project, &Style) -> Result<String>` and `cli::list::run(&Project, &Style, &mut impl Write)`; `agentsync list` and `agentsync ls` byte-identical to `lib/helpers/list.sh`.

- [x] **Step 1: Add the failing smoke tests to `tests/cli.rs`**

```rust
#[test]
fn list_works_without_a_project_config() {
    let dir = tempfile::tempdir().unwrap();
    agentsync()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  AgentSync Tools\n"))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("  0 of 13 enabled\n"))
        .stdout(predicate::str::contains("Enable a tool:"));
}

#[test]
fn ls_is_an_alias_for_list() {
    let dir = tempfile::tempdir().unwrap();
    agentsync()
        .current_dir(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("  AgentSync Tools\n"));
}

#[test]
fn list_counts_configured_tools_and_honours_the_repo_root_variable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
    std::fs::write(
        dir.path().join(".ai/agent_sync.yaml"),
        "tools:\n  enabled:\n    - claude\n",
    )
    .unwrap();
    agentsync()
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  1 of 13 enabled\n"))
        .stdout(predicate::str::contains("Customize a tool:"))
        .stdout(predicate::str::contains("Enable a tool:").not());
}
```

- [x] **Step 2: Run them, confirm they fail**

Run: `cargo test --test cli list`
Expected: 3 failures, clap reports `unrecognized subcommand 'list'`.

- [x] **Step 3: Write `src/style.rs`**

```rust
//! Colour helpers matching `lib/helpers/cli_colors.sh`: decided once from
//! stdout being a terminal and `NO_COLOR` being unset or empty.

use std::io::IsTerminal;

#[derive(Clone, Copy, Debug)]
pub struct Style {
    enabled: bool,
}

impl Style {
    pub fn for_stdout() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Self {
            enabled: std::io::stdout().is_terminal() && !no_color,
        }
    }

    pub const fn plain() -> Self {
        Self { enabled: false }
    }

    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }

    pub fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }

    pub fn cyan(&self, s: &str) -> String {
        self.wrap("36", s)
    }

    pub fn yellow(&self, s: &str) -> String {
        self.wrap("33", s)
    }

    pub fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }

    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
}

/// Left-align `s` in `width` cells the way Bash `printf '%-Ns'` does: escape
/// bytes of a styled string count, so coloured columns drift exactly as they
/// do today (design spec, "Known quirks", item 6).
pub fn pad_right(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_style_returns_the_text_unchanged() {
        assert_eq!(Style::plain().red("x"), "x");
    }

    #[test]
    fn enabled_style_wraps_with_the_bash_escape_codes() {
        let style = Style { enabled: true };
        assert_eq!(style.bold("x"), "\x1b[1mx\x1b[0m");
        assert_eq!(style.dim("x"), "\x1b[2mx\x1b[0m");
        assert_eq!(style.green("x"), "\x1b[32mx\x1b[0m");
    }

    #[test]
    fn padding_counts_every_character_including_escapes() {
        assert_eq!(pad_right("ab", 4), "ab  ");
        assert_eq!(pad_right("abcdef", 4), "abcdef");
        assert_eq!(pad_right(&Style { enabled: true }.dim("ab"), 12).len(), 12);
    }
}
```

- [x] **Step 4: Write `src/cli/list.rs`**

```rust
//! `agentsync list`: the tool catalog with per-project status, byte for byte
//! the table `lib/helpers/list.sh` prints.

use std::collections::BTreeSet;
use std::io::Write;

use crate::style::{Style, pad_right};
use crate::{Error, catalog, payload, project::Project, tool::Tool};

const RESOURCES: [(&str, &str); 3] = [("hooks", "H"), ("mcp", "M"), ("settings", "S")];

pub fn run(project: &Project, style: &Style, out: &mut impl Write) -> Result<(), Error> {
    let report = render(project, style)?;
    out.write_all(report.as_bytes()).map_err(|e| Error::io("<stdout>", e))
}

pub fn render(project: &Project, style: &Style) -> Result<String, Error> {
    let enabled = project.enabled_tools()?;
    let customized: BTreeSet<String> = project.user_override_tools()?.into_iter().collect();
    let all: BTreeSet<String> = catalog::base_tools()
        .into_iter()
        .chain(customized.iter().cloned())
        .collect();

    let mut text = String::from("\n");
    text.push_str(&style.bold("  AgentSync Tools"));
    text.push('\n');
    text.push_str(&style.dim(
        "  ● enabled   ○ available   ★ tool override   H M S = hooks/mcp/settings (· = base only, * = override, ~ = legacy override)",
    ));
    text.push('\n');
    text.push_str(&style.dim("  Columns: name · slug (use in commands) · status · resources"));
    text.push_str("\n\n");

    let mut enabled_count = 0usize;
    let mut customized_count = 0usize;
    let mut payload_override_count = 0usize;
    for slug in &all {
        let tool = Tool::load(project, slug)?;
        let (marker, status) = if enabled.contains(slug) {
            enabled_count += 1;
            (style.green("●"), style.dim("enabled"))
        } else {
            (style.dim("○"), style.dim("available"))
        };
        let star = if customized.contains(slug) {
            customized_count += 1;
            style.yellow("★")
        } else {
            " ".to_string()
        };
        let (resources, has_override) = resources_column(project, &tool, style)?;
        if has_override {
            payload_override_count += 1;
        }
        text.push_str(&format!(
            "    {marker} {star}  {} {} {}  {resources}\n",
            pad_right(&tool.display_name(), 22),
            pad_right(&style.dim(slug), 13),
            pad_right(&status, 10),
        ));
    }

    text.push('\n');
    let mut summary = format!("{enabled_count} of {} enabled", all.len());
    if customized_count > 0 {
        summary.push_str(&format!(", {customized_count} tool override(s)"));
    }
    if payload_override_count > 0 {
        summary.push_str(&format!(", {payload_override_count} payload override(s)"));
    }
    text.push_str(&format!("  {summary}\n"));

    let shared_mcp = project.shared_mcp_path().is_file();
    if shared_mcp {
        let mut mcp_overrides = 0usize;
        for slug in &all {
            if payload::find_new_override(project, slug, "mcp")?.is_some() {
                mcp_overrides += 1;
            }
        }
        let mut hint = format!("  Shared MCP: {}", style.yellow(".ai/src/mcp.json"));
        if mcp_overrides > 0 {
            hint.push(' ');
            hint.push_str(&style.dim(&format!("(+ {mcp_overrides} per-tool override)")));
        }
        text.push_str(&hint);
        text.push('\n');
    }

    text.push('\n');
    if enabled_count == 0 {
        text.push_str(&format!("  Enable a tool:     {}\n", style.cyan("agentsync enable <slug>")));
    }
    text.push_str(&format!(
        "  Customize a tool:  {}\n",
        style.cyan("agentsync customize <slug> [<resource>]")
    ));
    if !shared_mcp {
        text.push_str(&format!(
            "  Add MCP server:    {}\n",
            style.cyan("agentsync add mcp <server> --command …")
        ));
    }
    text.push_str(&format!("  Sync outputs:      {}\n", style.cyan("agentsync sync")));
    text.push('\n');
    Ok(text)
}

/// One cell per payload resource (`H*`, `M~`, `S `, or `· `, each followed by
/// a space) and whether any override, new layout or legacy, was found.
fn resources_column(project: &Project, tool: &Tool, style: &Style) -> Result<(String, bool), Error> {
    let mut cells = String::new();
    let mut has_override = false;
    for (resource, letter) in RESOURCES {
        let cell = if payload::find_new_override(project, &tool.slug, resource)?.is_some() {
            has_override = true;
            style.yellow(&format!("{letter}*"))
        } else if payload::legacy_override_path(project, tool, resource).is_some_and(|p| p.is_file()) {
            has_override = true;
            style.yellow(&format!("{letter}~"))
        } else if tool.base_payload(resource).is_some() {
            style.dim(&format!("{letter} "))
        } else {
            style.dim("· ")
        };
        cells.push_str(&cell);
        cells.push(' ');
    }
    Ok((cells, has_override))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_fresh_project_renders_the_bash_table_shape() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let text = render(&project, &Style::plain()).unwrap();
        assert!(text.starts_with("\n  AgentSync Tools\n  ● enabled"));
        assert!(text.contains("    ○    Claude Code            claude        available   ·  M  S  \n"));
        assert!(text.contains("    ○    Zed                    zed           available   ·  ·  S  \n"));
        assert!(text.contains("\n  0 of 13 enabled\n\n  Enable a tool:     agentsync enable <slug>\n"));
        assert!(text.contains("  Add MCP server:    agentsync add mcp <server> --command …\n"));
        assert!(text.ends_with("  Sync outputs:      agentsync sync\n\n"));
    }

    #[test]
    fn overrides_and_shared_mcp_change_markers_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/agent_sync.yaml", "tools:\n  enabled: [claude]\n");
        write(dir.path(), ".ai/src/tools/cursor.yaml", "name: \"My Cursor\"\n");
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        write(dir.path(), ".ai/src/mcp/claude.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        write(dir.path(), ".ai/src/tools/kimi/mcp.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        let text = render(&project, &Style::plain()).unwrap();
        assert!(text.contains("    ●    Claude Code            claude        enabled     ·  M~ S  \n"));
        assert!(text.contains("    ○ ★  My Cursor              cursor        available   H* M  ·  \n"));
        assert!(text.contains("    ○    Kimi Code              kimi          available   ·  M* ·  \n"));
        assert!(text.contains("\n  1 of 13 enabled, 1 tool override(s), 3 payload override(s)\n"));
        assert!(text.contains("  Shared MCP: .ai/src/mcp.json (+ 1 per-tool override)\n"));
        assert!(!text.contains("Enable a tool:"));
        assert!(!text.contains("Add MCP server:"));
    }
}
```

The two unit tests assert exact rows. If one fails on spacing, compare against `AGENTSYNC_NATIVE=0 agentsync list | cat -A` on the same fixture before touching the format string: the Bash row is `printf "    %s %s  %-22s %-13s %-10s  %s\n"`.

- [x] **Step 5: Add `List` to `src/cli/mod.rs`**

```rust
pub mod list;

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
}
```

- [x] **Step 6: Dispatch `List` in `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::project::Project;
use agentsync::style::Style;
use agentsync::{Error, engine_version};
use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            // Bash decides colour from stdout even for stderr lines; same here.
            eprintln!("{}: {e}", Style::for_stdout().red("Error"));
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), Error> {
    guard_engine_version()?;
    // The Bash CLI accepts these flags as commands; clap's own version flag is off.
    if matches!(args.first().and_then(|a| a.to_str()), Some("--version" | "-v")) {
        return print_version();
    }
    let cli = Cli::parse_from(std::iter::once(OsString::from("agentsync")).chain(args));
    match cli.command {
        Command::Version => print_version(),
        Command::List => {
            let project = Project::discover()?;
            let mut out = std::io::stdout().lock();
            cli::list::run(&project, &Style::for_stdout(), &mut out)
        }
    }
}

fn print_version() -> Result<(), Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version()).map_err(|e| Error::io("<stdout>", e))
}

/// `bin/agentsync.sh` passes its own VERSION so a binary left behind by an
/// older checkout can never answer for a newer engine.
fn guard_engine_version() -> Result<(), Error> {
    let Some(engine) = std::env::var_os("AGENTSYNC_ENGINE_VERSION") else {
        return Ok(());
    };
    let engine = engine.to_string_lossy().into_owned();
    if engine.is_empty() || engine == engine_version() {
        return Ok(());
    }
    Err(Error::StaleBinary {
        binary: engine_version().to_string(),
        engine,
    })
}
```

Add `pub mod style;` to `src/lib.rs` between `pub mod project;` and `pub mod tool;`.

- [x] **Step 7: Run the Rust tests, confirm green**

Run: `cargo test`
Expected: every test passes, including the 3 new smoke tests and the 2 `list` unit tests.

- [x] **Step 8: Declare `list` ported in `bin/agentsync.sh`**

Change the list to:

```bash
_NATIVE_COMMANDS=" version --version -v list ls "
```

- [x] **Step 9: Add the `--help` guard to `tests/native_dispatch.bats`**

```bash
@test "native: --help for a ported command still prints the Bash usage" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync list --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"COMMANDS"* ]]
    [[ "$output" != *"native:"* ]]
}
```

- [x] **Step 10: Run the bats files in both modes, confirm green**

```bash
cargo build --release
bats tests/list.bats tests/cli.bats tests/native_dispatch.bats
AGENTSYNC_NATIVE=1 bats tests/list.bats tests/cli.bats tests/native_dispatch.bats
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: 8 + 8 + 8 tests pass in each mode; ShellCheck exits 0.

- [x] **Step 11: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/ tests/cli.rs bin/agentsync.sh tests/native_dispatch.bats
git commit -m "feat(native): port list"
```

---

### Task 7: Parity Suite and CI

**Files:**
- Create: `tests/native_parity.bats`
- Modify: `.github/workflows/ci.yaml` (`native` job runs the ported bats files against the binary)

**Interfaces:**
- Consumes: the binary, `_native_try`, `seed_project`/`clone_seed`/`enable_tools` from `tests/test_helper.bash`.
- Produces: `assert_parity <args...>` (a bats helper local to the file) and one fixture per `list` situation; later phases add tests here for each ported command.

- [x] **Step 1: Write `tests/native_parity.bats`**

```bash
#!/usr/bin/env bats
# Bash and native answers for a ported command must match byte for byte.
# Skips when no binary is built: `cargo build --release` first.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }

setup() {
    clone_seed
    NATIVE_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
    if [[ ! -x "$NATIVE_BIN" ]] && [[ -x "$NATIVE_BIN.exe" ]]; then
        NATIVE_BIN="$NATIVE_BIN.exe"
    fi
    [[ -x "$NATIVE_BIN" ]] || skip "no native binary at $NATIVE_BIN"
    export AGENTSYNC_NATIVE_BIN="$NATIVE_BIN"
}

teardown() { teardown_test_project; }

_run_engine() {
    local mode="$1"
    shift
    AGENTSYNC_NATIVE="$mode" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" "$@"
}

# Usage: assert_parity <agentsync args...>
# Fails with a diff when stdout+stderr differ, or when the exit status differs.
assert_parity() {
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

@test "parity: version and its flags" {
    assert_parity version
    assert_parity --version
    assert_parity -v
}

@test "parity: list on a fresh project" {
    assert_parity list
    assert_parity ls
}

@test "parity: list with enabled tools" {
    enable_tools claude cursor
    assert_parity list
}

@test "parity: list with a payload override in the per-tool layout" {
    mkdir -p .ai/src/tools/cursor
    echo '{}' > .ai/src/tools/cursor/hooks.json
    assert_parity list
}

@test "parity: list with a legacy flat-layout override" {
    mkdir -p .ai/src/mcp
    echo '{}' > .ai/src/mcp/claude.json
    assert_parity list
}

@test "parity: list with a shared MCP source and one per-tool override" {
    echo '{}' > .ai/src/mcp.json
    mkdir -p .ai/src/tools/kimi
    echo '{}' > .ai/src/tools/kimi/mcp.json
    assert_parity list
}

@test "parity: list with a custom tool enabled the legacy way" {
    mkdir -p .ai/src/tools
    printf 'name: "My Tool"\nenabled: true\n' > .ai/src/tools/mytool.yaml
    assert_parity list
}

@test "parity: list with a profile variant tool" {
    mkdir -p .ai/src/tools
    printf 'base: claude\nprofile_home: ".claude-hub"\n' > .ai/src/tools/claude-hub.yaml
    assert_parity list
}

@test "parity: list without a .ai directory" {
    rm -rf .ai
    assert_parity list
}
```

- [x] **Step 2: Run it against the built binary, confirm green**

```bash
cargo build --release
bats tests/native_parity.bats
```

Expected: 9 tests pass. A failure prints a unified diff of the two outputs; fix the native side, never the fixture.

- [x] **Step 3: Run it without a binary, confirm it skips**

Run: `AGENTSYNC_NATIVE_BIN=/nonexistent bats tests/native_parity.bats`
Expected: 9 tests reported as skipped, exit 0.

- [x] **Step 4: Extend the `native` CI job**

Append these steps to the `native` job in `.github/workflows/ci.yaml`, after `cargo build --release`:

```yaml
      - name: Install bats-core
        if: runner.os != 'Windows'
        shell: bash
        run: |
          if [[ "$RUNNER_OS" == "Linux" ]]; then
            sudo apt-get install -y bats
          else
            brew install bats-core
          fi
      # Windows joins in Phase 5, when the binary is the entry point: under
      # Git Bash the POSIX paths in AGENTSYNC_REPO_ROOT and TMPDIR do not reach
      # a native executable.
      - name: Ported commands through the Bash suite
        if: runner.os != 'Windows'
        shell: bash
        env:
          TERM: xterm
          AGENTSYNC_NATIVE: "1"
        run: bats tests/cli.bats tests/list.bats tests/native_dispatch.bats tests/native_parity.bats --tap
```

- [x] **Step 5: Run the whole suite in both modes one last time**

```bash
bats --jobs 4 tests/ --tap | tail -3
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap | tail -3
```

Expected: `1..743` (725 + 1 list regression + 8 dispatcher + 9 parity) with no `not ok` in both runs. The second run proves an unported command is unaffected by a present binary.

- [x] **Step 6: Commit**

```bash
git add tests/native_parity.bats .github/workflows/ci.yaml
git commit -m "test(native): diff Bash against native output for ported commands"
```

---

### Task 8: Developer Documentation

**Files:**
- Modify: `README.md` (`## Development`)
- Modify: `.ai/src/AGENTS.md` (`## Tech Stack`, `## Approach` step 4)

**Interfaces:**
- Consumes: everything above.
- Produces: the two places a contributor reads before touching the engine.

- [x] **Step 1: Add a subsection to `README.md` under `## Development`, before `## License`**

````markdown
### Native engine

Commands are moving one by one to a Rust binary
(`docs/specs/2026-09-12-rust-migration-design.md`). The Bash CLI hands a ported
command to the binary when one is available:

```bash
cargo build --release                 # target/release/agentsync
agentsync list                        # served natively when the binary exists
AGENTSYNC_NATIVE=0 agentsync list     # force the Bash implementation
AGENTSYNC_NATIVE=1 bats tests/        # run the suite against the binary for ported commands
```

`cargo test` covers the Rust side; `tests/native_parity.bats` diffs Bash
against native output for every ported command.
````

- [x] **Step 2: Update `.ai/src/AGENTS.md`**

In `## Tech Stack`, after the `**Entry point**` line:

```markdown
- **Native engine**: Rust crate at the repo root (`src/`), templates embedded from `lib/templates/`; `bin/agentsync.sh` delegates the commands listed in `_NATIVE_COMMANDS` to `target/release/agentsync`
```

In `## Approach`, replace step 4 with:

```markdown
4. **Verify** — Run `shellcheck -x -S warning -e SC1091` on changed scripts and `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` on changed Rust. Run `bats tests/` for the full suite, or target specific `.bats` files; a ported command also needs `AGENTSYNC_NATIVE=1 bats <its file>` and a case in `tests/native_parity.bats`.
```

Then regenerate the local agent files: `bash bin/agentsync.sh sync` (outputs are gitignored in this repository).

- [x] **Step 3: Commit**

```bash
git add README.md .ai/src/AGENTS.md
git commit -m "docs: describe the native engine and how to run it"
```

---

## Completion

Before reporting Phase 1 done, produce the completion receipt from `verification.md`:

- Each Global Constraint mapped to the file that satisfies it.
- Fresh output of: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`, `shellcheck` over the shell entry points, `bats --jobs 4 tests/ --tap` in both `AGENTSYNC_NATIVE` modes, and the `native` CI job green on all three runners.
- The language decision gate from the design spec: how long Tasks 1–7 took and whether Rust velocity is acceptable before Phase 2's plan is written.

---

## Run log

### 2026-09-12 — Task 0 blocked at Step 1 (no Rust toolchain)
- Commits: `docs(native): log run 2026-09-12`
- Verified: `git switch -c feat/native-engine-phase-1 main` → `Switched to a new branch 'feat/native-engine-phase-1'`, `git status --short` empty (Task 0 Step 2 done); `command -v cargo` → not found, so Task 0 Step 1 is unmet and Step 3's baseline was not run.
- Plan amended: none. The design spec's `Status:` line moved from `Proposed` to `In progress since 2026-09-12` with this first commit on the phase branch.
- Next: Task 0 Step 1 — install the toolchain, then Step 3's baseline (`bats --jobs 4 tests/ --tap`, ShellCheck over the shell entry points), then Task 0b Step 1.
- Blocker: `command -v cargo` prints nothing. Needs, on this machine: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default` then `. "$HOME/.cargo/env"`, until `cargo --version && rustc --version` both report 1.85 or newer. This command never installs a toolchain.

### 2026-09-12 — Task 0 done, Task 0b done
- Commits: `fix(list): survive a tool override without enabled: true`
- Verified: toolchain installed by the user, `cargo 1.98.1 (797e8a9bc 2026-08-05)` and `rustc 1.98.1 (48a229cea 2026-09-01)`, both above MSRV 1.85 (Step 1). `bats --jobs 4 tests/ --tap` → `1..725`, no `not ok`, exit 0; `shellcheck -x -S warning -e SC1091` over the six shell entry points → exit 0 (Step 3). Task 0b: the new `tests/list.bats` case failed red at `[ "$status" -eq 0 ]` before the fix and passes after; `bats tests/list.bats tests/doctor.bats tests/enable.bats --tap` → 55 ok, 0 not ok (8 + 34 + 13 as planned); `shellcheck` on `lib/helpers/tool_resolver.sh` → exit 0.
- Plan amended: none.
- Next: Task 1 Step 1 — create `Cargo.toml` for the crate scaffold. Note for the next run: `cargo` is not on the agent shell's `PATH` (the profile snapshot predates the install); invoke it as `/Users/yelamanyelmuratov/.cargo/bin/cargo` or prepend `/Users/yelamanyelmuratov/.cargo/bin` to `PATH`.
- Blocker: none.

### 2026-09-12 — Task 1 done
- Commits: `feat(native): scaffold the Rust engine with a version command`
- Verified: `cargo test` before the modules existed failed to compile `tests/cli.rs` (`environment variable CARGO_BIN_EXE_agentsync not defined`, no binary target yet) — red for the right reason; after Steps 4–7, `cargo test` → 4 passed, 0 failed; `cargo fmt --all --check` → exit 0; `cargo clippy --all-targets -- -D warnings` → exit 0; `cargo build --release` → `target/release/agentsync` prints `agentsync v0.35.2`, byte-identical to `bash bin/agentsync.sh version`; `git check-ignore -v target` → `.gitignore:34:/target/`.
- Plan amended: none. Two mechanical deviations from the pasted snippets, both required by `rustfmt` and reported: the `StaleBinary` `#[error(...)]` string wraps onto its own line, and the `matches!` in `run()` wraps its arguments. No behaviour change; `cargo fmt --all --check` is clean.
- Next: Task 2 Step 1 — write `tests/native_dispatch.bats`, the failing tests for the dispatcher in `bin/agentsync.sh`.
- Blocker: none. Note for the next run: `cargo` needs the sandbox disabled to write `~/.cargo/registry`; it is not in the sandbox write allowlist.

### 2026-09-12 — Task 2 done
- Commits: `feat(cli): delegate ported commands to the native engine`; also in this run, outside the plan, `f33223f docs(native): add phase 7 to retire bats and name the shell floor` — the user set the target at no Bash left, so the 43 `.bats` files got their own phase after Phase 6, and the two scripts that survive every phase are now named in the spec with the reason each cannot be a binary.
- Verified: `tests/native_dispatch.bats` failed 1, 2, 3 and 7 before the dispatcher existed and 4, 5, 6 passed by accident, exactly as Step 2 predicts; after Step 4, `bats tests/native_dispatch.bats tests/cli.bats` → 15 ok, 0 not ok; `shellcheck -x -S warning -e SC1091 bin/agentsync.sh` → exit 0; `AGENTSYNC_NATIVE=1 bats tests/cli.bats` → 8 ok, answered by `target/release/agentsync`; `AGENTSYNC_NATIVE=1 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh version` → `agentsync v0.35.2`; full suite `bats --jobs 4 tests/ --tap` → bats exit 0, plan `1..733`, 733 ok, 0 not ok.
- Plan amended: Step 4's `main` insertion. The snippet's bare `_native_try "$@"` aborts `main` under `set -euo pipefail`, because falling through to Bash is a `return 1` — `bats tests/cli.bats` went 0/8 on the first write, every command exiting 1 with no output. Now `_native_try "$@" || true`, with the reason in the plan. Step 4's comment block was also reworded: the comment gate reads `#   AGENTSYNC_NATIVE=0   always Bash` as commented-out code, so the env contract is one prose sentence and the binary lookup order is left to `_native_bin` below.
- Next: Task 3 Step 1 — write `src/yaml_subset.rs` with its failing unit tests, mirroring `lib/helpers/yaml.sh`.
- Blocker: none.

### 2026-09-12 — Task 3 done
- Commits: `feat(native): read the AgentSync YAML subset`
- Verified: `cargo test yaml_subset` → 15 passed, 0 failed, including the quirk test `an_empty_block_key_takes_the_next_dash_list_like_bash_does`; `cargo test` → 15 + 4 passed; `cargo fmt --all --check` → exit 0 after `cargo fmt --all` reflowed two expressions in `split_key` and `unwrap_quoted`; `cargo clippy --all-targets -- -D warnings` → exit 0. Step 4's Bash reference, read from the shipped Cursor template, is the literal `---\nglobs: '**/*'\nalwaysApply: true\n---` and an empty include list; a throwaway unit test over `include_str!("../lib/templates/tools/cursor.yaml")` confirmed `value` and `list` return the same, then was removed — Task 4 embeds the catalog with `include_dir!`, so a test pinned to a repo-relative path would be churn.
- Plan amended: none. Before writing, the port was read against `lib/helpers/yaml.sh:13-30` (`_yaml_normalize_scalar_reply`), `:40-119` (`parse_yaml_value_r`) and `:133-235` (`parse_yaml_list`); the only divergence found is that Bash `[[:space:]]` includes a vertical tab and Rust's `is_ascii_whitespace` does not, which no config exercises.
- Next: Task 4 Step 1 — write `src/catalog.rs`, the `include_dir!` embedding of `lib/templates/`.
- Blocker: none.

### 2026-09-12 — Task 4 done
- Commits: `feat(native): resolve project config and layered tool values`
- Verified: `cargo test` → 28 unit (15 `yaml_subset`, 3 `catalog`, 5 `project`, 5 `tool`) + 4 integration, 0 failed, the count Step 5 predicts; `cargo fmt --all --check` → exit 0 after `cargo fmt --all` expanded the one-line `if` in `display_name`; `cargo clippy --all-targets -- -D warnings` → exit 0. The layering assertions were confirmed against the Bash resolver rather than read off it: in a temp project with `.ai/src/tools/claude.yaml` holding only `base: cursor`, `get_tool_value claude targets.rules.extension` → empty and `targets.rules.dest` → `.claude/rules` (a shipped base answers even empty and blocks the variant fallback), and for a `claude-hub` variant `targets.rules.dest` → `.claude/rules` while `name` → empty, which is why `display_name` falls back to the slug. Catalog facts checked against `lib/templates/`: 13 tool YAMLs plus `_TEMPLATE`, `settings/claude.json` present, no `hooks/zed`.
- Plan amended: Step 1's `file_name` helper. `fn file_name(file: &File<'_>) -> Option<&str>` does not compile — E0106, the return borrows from one of the `File`'s two lifetimes and the signature does not say which. Now `fn file_name<'a>(file: &'a File<'_>) -> Option<&'a str>`, since `path()` borrows from the reference and not from the embedded `'static` bytes.
- Next: Task 5 Step 1 — write `src/payload.rs` with its tests.
- Blocker: none.

### 2026-09-12 — Task 5 done
- Commits: `feat(native): locate payload overrides`
- Verified: `cargo test payload` → both `payload::` tests pass; `cargo test` → 30 unit + 4 integration, 0 failed; `cargo fmt --all --check` → exit 0 with no reflow needed; `cargo clippy --all-targets -- -D warnings` → exit 0. The legacy paths were confirmed against Bash with `REPO_ROOT=/tmp/proj`: `_payload_override_legacy_path claude settings` → `/tmp/proj/.ai/src/settings/claude.json`, `claude hooks` → empty, `codex settings` → `/tmp/proj/.ai/src/settings/codex.toml` — the three values the unit test asserts.
- Plan amended: none. Read against `lib/helpers/tool_resolver.sh:257-269` (`_find_new_payload_override`, glob `<resource>.*` with the first existing file winning) and `:272-280` (`_payload_override_legacy_path`, extension taken from `_find_base_payload` at `:214-236`, which falls back to the `base:` tool exactly as `Tool::base_payload` does).
- Next: Task 6 Step 1 — write `src/style.rs`, the mirror of `lib/helpers/cli_colors.sh`.
- Blocker: none.

### 2026-09-12 — Task 6 done, `list` is the first ported command
- Commits: `feat(native): port list`
- Verified: the three smoke tests failed first with clap's `unrecognized subcommand 'list'`, exit 2, as Step 2 predicts. After the port, `cargo test` → 35 unit + 7 integration, 0 failed, including the two `cli::list` tests that assert exact table rows; `cargo fmt --all --check` → exit 0 after `cargo fmt --all` reflowed four assertions and one `push_str`; `cargo clippy --all-targets -- -D warnings` → exit 0. `cargo build --release`, then `bats tests/list.bats tests/cli.bats tests/native_dispatch.bats --tap` → `1..24`, 24 ok in Bash mode and 24 ok under `AGENTSYNC_NATIVE=1`; `shellcheck -x -S warning -e SC1091 bin/agentsync.sh` → exit 0. Byte parity on a real fixture, this repository itself (25 lines, `2 of 13 enabled, 1 payload override(s)`, shared-MCP line present): `diff <(AGENTSYNC_NATIVE=0 … list) <(AGENTSYNC_NATIVE=1 … list)` → no output.
- Plan amended: none. `lib/helpers/list.sh:120` (`printf "    %s %s  %-22s %-13s %-10s  %s\n"`) and `lib/helpers/cli_colors.sh:5-13` were read before writing; the escape codes and the `NO_COLOR` rule (colour only when stdout is a terminal and `NO_COLOR` is unset or empty) match the port.
- Next: Task 7 Step 1 — write `tests/native_parity.bats`.
- Blocker: none.

### 2026-09-12 — Task 7 done
- Commits: `test(native): diff Bash against native output for ported commands`
- Verified: `bats tests/native_parity.bats --tap` → `1..9`, 9 ok against the release binary; `AGENTSYNC_NATIVE_BIN=/nonexistent bats tests/native_parity.bats` → all 9 skipped, exit 0, so a checkout without a build stays green. Whole suite both ways: `bats --jobs 4 tests/ --tap` → bats exit 0, `1..743`, 743 ok, 0 not ok, and `AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap` → the same, which is the proof that a present binary leaves every unported command alone. 743 = 725 baseline + 1 `list` regression + 8 dispatcher + 9 parity, the count Step 5 predicts.
- Plan amended: the last two fixtures in Step 1. `printf … > .ai/src/tools/mytool.yaml` and the `claude-hub.yaml` twin failed with `No such file or directory` — `clone_seed` leaves no `.ai/src/tools/`, which the earlier fixtures happen to create with their own `mkdir -p`. Both now create the directory first. A setup bug in the fixture, not an engine difference.
- Next: Task 8 Step 1 — add the `### Native engine` subsection to `README.md` under `## Development`.
- Blocker: none.

### 2026-09-13 — Task 8 done, all 61 plan steps checked
- Commits: `docs: describe the native engine and how to run it`
- Verified: `bash bin/agentsync.sh sync` → `Synced 2/13 tools (11 skipped)`, backup `.ai/backups/20260912T190120Z-sync-6037`; the regenerated `CLAUDE.md` carries both new lines. The first attempt failed under the agent sandbox with `Can't create '.claude/commands/…': Operation not permitted` and rolled back whole — `[ERROR] Could not back up sync targets; no files were changed` — which is the transaction behaving as specified.
- Plan amended: Step 3's `git add` list. The sync updates `.ai/.sync-manifest`, which is tracked here, and its diff is exactly the two hashes for `AGENTS.md` and `CLAUDE.md`; leaving it out would leave `agentsync check` reporting drift, so the commit adds it too.
- Next: close the phase. Append the `## Completion receipt` per the plan's `## Completion` section — every Global Constraint mapped to its file, fresh runs of `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`, ShellCheck over the shell entry points, and `bats --jobs 4 tests/ --tap` in both `AGENTSYNC_NATIVE` modes — then answer the language decision gate and stop for the user's verdict. The `native` CI job has not run yet: the branch is local, so that line of the receipt stays open until it is pushed.
- Blocker: none.

### 2026-09-13 — phase closed
- Commits: `docs(native): close phase 1`
- Verified: everything in the completion receipt below, all run fresh after the last code commit.
- Plan amended: none.
- Next: the user's verdict on the language decision gate, then `docs/plans/…-phase-2-….md` written from `.ai/src/commands/native-phase-plan.md`. The branch is not pushed and not merged; both are the user's call. The two toolchain caveats the earlier entries carry are fixed and no longer apply: `~/.cargo` is in the agent sandbox's write allowlist (with `~/.cargo/bin` denied), and `~/.zshrc` now sources `~/.cargo/env`, which rustup had written only to `~/.profile` — a file zsh never reads. `cargo` runs sandboxed and resolves on `PATH`.
- Blocker: the `native` CI job has never run — the branch is local. The receipt records that line as open.

---

## Completion receipt

Phase 1 closed 2026-09-13. Every checkbox above is ticked; `version`, `--version`,
`-v`, `list` and `ls` are served by the Rust binary when one is built.

### Global Constraints

| Constraint | Satisfied by | Evidence |
| --- | --- | --- |
| `.ai/src/` stays the source of truth; nothing writes under a user project | `src/` has no write outside `#[cfg(test)]` | `grep -rn 'fs::write\|create_dir_all' src/` returns only the `write()` helpers in the three test modules (`project.rs:118`, `payload.rs:68`, `cli/list.rs:152`) |
| No binary ships to users; without one every command runs in Bash as in 0.35.2 | `bin/agentsync.sh:276-330` (`_native_try`, `_native_bin`) | `install.sh` untouched this phase; `native: without AGENTSYNC_NATIVE a missing binary falls back to Bash` in `tests/native_dispatch.bats`; `AGENTSYNC_NATIVE_BIN=/nonexistent bats tests/native_parity.bats` skips all 9 and exits 0 |
| `bin/agentsync.sh` stays Bash 3.2-compatible and ShellCheck-clean | `bin/agentsync.sh` | the added code uses only `[[ ]]`, `local`, `for`, `echo` — no associative array, nameref or `mapfile`; `shellcheck -x -S warning -e SC1091` → exit 0 |
| A ported command matches Bash byte for byte off a terminal | `tests/native_parity.bats` (9 fixtures) | `bats tests/native_parity.bats` → `1..9`, 9 ok; `diff` of `list` output on this repository's own `.ai/src/` (25 lines, `2 of 13 enabled, 1 payload override(s)`) → empty |
| Terminal output uses the escape codes of `cli_colors.sh` | `src/style.rs` | `enabled_style_wraps_with_the_bash_escape_codes` asserts `\x1b[1m`, `\x1b[2m`, `\x1b[32m`, the codes at `lib/helpers/cli_colors.sh:8-13`; the `NO_COLOR` rule matches `:6`. Not exercised on a real terminal — bats never sees colour; deferred below |
| Rust: `unsafe_code = "forbid"`, fmt and clippy clean, no YAML crate | `Cargo.toml:24`, `src/yaml_subset.rs` | dependencies are clap, include_dir, thiserror only; `yaml_subset` mirrors `lib/helpers/yaml.sh:13-235` |
| `VERSION` is the only version source; `Cargo.toml` stays `0.0.0` | `src/lib.rs:17`, `Cargo.toml:5` | `include_str!("../VERSION")` in the crate and in `tests/cli.rs:5`; `a_stale_binary_refuses_to_run` covers the `AGENTSYNC_ENGINE_VERSION` guard |
| Accepted deviations recorded in the design spec | `docs/specs/2026-09-12-rust-migration-design.md:337-345` | three lines: clap's exit 2 on stray arguments, byte-order slug sort, and `\r\n` read as `\n` |
| Conventional Commits, scope `native`, no attribution trailers | the 11 commits on this branch | `git log --format='%b' main..HEAD` matches no `Co-Authored-By`, `Generated with`, or assistant signature |

### Fresh verification, 2026-09-13, macOS aarch64, rustc 1.98.1

| Command | Result |
| --- | --- |
| `cargo test` | 35 unit + 7 integration passed, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all --check` | exit 0 |
| `cargo build --release` | `Finished release profile` in 7.17 s |
| `shellcheck -x -S warning -e SC1091` over `bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh` | exit 0 |
| `shellcheck` over `lib/templates/guard/claude.sh` | exit 0 |
| `bats --jobs 4 tests/ --tap` | bats exit 0, `1..743`, 743 ok, 0 not ok |
| `AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap` | bats exit 0, `1..743`, 743 ok, 0 not ok |

743 = 725 at the 0.35.2 baseline + 1 `list` regression (Task 0b) + 8 dispatcher + 9 parity.

### Timings

The phase changed neither `sync` nor `check`, so the 13-tool fixture timing the
spec asks for is not due. `list` on this repository, best of three:

| Path | Wall time |
| --- | --- |
| `AGENTSYNC_NATIVE=0 … list` (Bash) | 0.54 s |
| `AGENTSYNC_NATIVE=1 … list` (Bash dispatcher, then binary) | 0.02 s |
| `target/release/agentsync list` (binary alone) | under 0.005 s |

The dispatcher costs about 20 ms — that is Bash starting, resolving its version
and sourcing `cli_colors`, `resolve` and `update` before it can delegate. It
disappears at the Phase 5 cutover, when the binary becomes the entry point.

### Skipped, deferred, open

- **The `native` CI job has never run.** The branch is local and unpushed, so
  "green on all three runners" is unverified. The job is defined in
  `.github/workflows/ci.yaml` and runs fmt, clippy, `cargo test` and a release
  build on Linux, macOS and Windows, plus the four ported bats files under
  `AGENTSYNC_NATIVE=1` on the two Unix runners.
- **Native bats on Windows is deferred to Phase 5 by design**, recorded in the
  workflow: under Git Bash the POSIX paths in `AGENTSYNC_REPO_ROOT` and
  `TMPDIR` never reach a native executable. `cargo test` still runs there.
- **Colour on a real terminal is asserted by unit test only.** bats captures
  output, so the escape codes have not been compared against Bash on a tty. A
  `script`-driven check belongs in the phase that first prints colour a user
  is likely to see interactively.
- **`_NATIVE_COMMANDS` holds five entries**; every other command is Bash, which
  is the phase's intent, not a gap.

### Language decision gate

The spec makes Go the fallback if the maintainer's velocity in Rust is not
acceptable at this point. The evidence from this phase:

- Tasks 1–7 — crate, dispatcher, YAML reader, config and tool layering, payload
  discovery, `list`, parity suite — landed in seven commits between 23:01 and
  23:55 on 2026-09-12; the whole branch, from the toolchain install through the
  725-test baseline and the Bash prefactor, spans 22:11 to 00:01.
- Three defects surfaced, all in the plan rather than in the language: a missing
  lifetime annotation on `catalog::file_name` (E0106), a bare `_native_try` call
  that `set -e` turned into an abort, and two parity fixtures that wrote into a
  directory `clone_seed` does not create. The first is the only Rust-specific
  one, and the compiler named the fix.
- Nothing in the phase needed a borrow-checker fight, an explicit lifetime
  beyond that one, `unsafe`, or a dependency outside the three planned crates.
  The workload is what the spec predicted: files in, strings transformed,
  strings out.
- `Result` did the job it was chosen for: every filesystem call in `project.rs`
  and `payload.rs` has a named failure, where the Bash original silently
  returned empty — the `list` exit-1 bug fixed in Task 0b is exactly the class
  of defect the type system removes.

Recommendation: **stay on Rust**. The phase produced no evidence for the
fallback. Note the honest limit of this measurement: the work executed a plan
that already carried the design and most of the code, so it measures execution
velocity, not design-from-scratch velocity in Rust.

**Verdict, 2026-09-13: Rust stays.** The gate is answered, the Go fallback is
closed in the design spec, and Phase 2 can be planned.
