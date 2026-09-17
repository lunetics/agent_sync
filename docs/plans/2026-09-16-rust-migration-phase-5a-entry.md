# Rust Migration Phase 5a: Native `help` and the Dispatcher's Own Answers

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the binary answer, byte for byte, what `bin/agentsync.sh` answers before it delegates: `print_usage` for `help`, `--help`, `-h`, an empty command, and no command at all; the usage for `--help` or `-h` right after `check`, `doctor`, `list`, `ls`, `show`, `diff`, `disable`, or `resolve`; and the `Unknown command` refusal with the usage on stderr and status 1. `_NATIVE_COMMANDS` gains `help --help -h`. This is the first of the five Phase 5 slices, and the one the cutover (5e) needs so that the binary can stand as the entry point.

**Architecture:** `src/cli/usage.rs` holds `print_usage` as `usage(&Style) -> String` (the 29 command rows padded to column 15 as the Bash literals are), `wants_usage(&[String]) -> bool` for `main`'s `${1:-help}` and its `--help` interception, and `unknown_command` for the `*)` arm. `src/main.rs` asks `wants_usage` right after the engine-version guard and refuses every first argument that no native arm answers just before clap parses. The seam stays the CLI process boundary, with one new helper: `assert_entry_parity` in `tests/native_parity.bats` runs Bash through the dispatcher and the binary with no dispatcher in front of it, and compares stdout and stderr apart, because through the dispatcher the interception and the refusal never reach the binary.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 5". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-4m-tail.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `help` and the refusal write nothing.
- No binary ships to users; without a binary every command runs in Bash. The only Bash change is the `_NATIVE_COMMANDS` line; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, and exit status, through the dispatcher and with the binary called directly, except for the accepted deviations. `version --help` and `check --only x --help` keep the Phase 1 deviation (clap refuses the argument with status 2 where Bash ignored it); the fixtures leave them out.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state. The binary spawns nothing new.
- `check_for_updates` and the project-format notice stay in the dispatcher: Phase 5d moves them with the release-based update banner.
- Every expected value was captured from Bash on 2026-09-16: `phase5a/entry_reference.sh` (31 argument shapes, 2468 lines, stdout and stderr apart) and `phase5a/entry_tty.sh` (3 scenarios on a pty through `script`, 738 lines of `od -c`). Both scripts, and `phase5a/native_suite.sh`, are reproduced in Task 1 Step 5 and Task 2 Step 2.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

1. **One task for the whole slice.** The usage, the interception, and the refusal share one module and one `main` edit, and `help` joins `_NATIVE_COMMANDS` in the same commit, as `feat(native): port help and the dispatcher's own answers`. Alternative: the refusal in its own task; rejected, because the refusal prints the usage.
2. **`update` and `release` are refused as unknown by the binary until 5b and 5d.** No native arm answers them yet; through the dispatcher neither reaches the binary, and the binary is not an entry point before 5e, so no user can see the message. Alternative: a dedicated "still served by Bash" message; rejected as text nobody would read and 5b and 5d would delete.
3. **The terminal notices stay in Bash.** `check_for_updates` runs in the dispatcher before `_native_try` and prints the format notice and the update banner, so the binary prints neither in 5a and a terminal run shows each once. 5d moves both, because the banner's cache and source change with the binary install.
4. **No new quirk or deviation.** The 31 shapes in `entry_reference.sh` and the three pty scenarios match byte for byte. **Recommended:** as listed.

## Module closure

```text
bin/agentsync.sh                 97-183   print_usage
                                 280      _NATIVE_COMMANDS
                                 328-351  main: ${1:-help}, the check_for_updates list (stays until 5d), the --help interception
                                 400-407  the version, help|--help|-h, and *) arms
lib/helpers/cli_colors.sh        5-13     _USE_COLORS, _bold _green _cyan _red _dim
tests/native_dispatch.bats       48-54    "an unported command never reaches the binary" names help, which this slice ports
```

Reused: `style::Style`, `cli::customize::put`, `engine_version`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in cli native_dispatch native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
grep -c '^@test' tests/cli.bats tests/native_dispatch.bats tests/native_parity.bats
```

Expected: the plan's latest commit; `285 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses; `8`, `8`, `65` cases.

---

### Task 1: Port `help`, the `--help` Interception, and the Unknown-Command Refusal

**Files:**
- Create: `src/cli/usage.rs`
- Modify: `src/style.rs`, `src/cli/mod.rs`, `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_dispatch.bats:48-54`
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/style.rs
impl Style { #[cfg(test)] pub(crate) const fn colored() -> Self; }
// src/cli/usage.rs
pub fn usage(style: &Style) -> String;
pub fn wants_usage(args: &[String]) -> bool;
pub fn unknown_command(command: &str, style: &Style, err: &mut dyn Write) -> Result<u8, Error>;
// src/main.rs
fn print_usage() -> Result<u8, Error>;
```

- [x] **Step 1: Parity fixtures, failing against the current binary**

Append to `tests/native_parity.bats`:

```bash

# ── help and the dispatcher's own answers ────────────────────────────────────
# bin/agentsync.sh answers these before it delegates, and the binary must answer
# them the same way once it is the entry point, so the native side here is the
# binary with no dispatcher in front of it. Stdout and stderr are compared apart.

# Usage: assert_entry_parity <agentsync args...>
assert_entry_parity() {
    local dir="$BATS_TEST_TMPDIR/entry" bash_rc=0 native_rc=0 stream
    mkdir -p "$dir"
    _run_engine 0 "$@" > "$dir/bash.out" 2> "$dir/bash.err" || bash_rc=$?
    "$NATIVE_BIN" "$@" > "$dir/native.out" 2> "$dir/native.err" || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs for [$*]: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    for stream in out err; do
        if ! cmp -s "$dir/bash.$stream" "$dir/native.$stream"; then
            echo "std$stream differs for [$*]" >&2
            diff "$dir/bash.$stream" "$dir/native.$stream" >&2 || true
            return 1
        fi
    done
}

@test "parity: help and a missing command print the usage like Bash" {
    assert_parity help
    assert_parity --help
    assert_parity -h
    assert_parity
    assert_parity ""
    assert_entry_parity
    assert_entry_parity ""
    assert_entry_parity help sync --bogus
    assert_entry_parity -h extra
}

@test "parity: --help after a command that does not parse it prints the usage like Bash" {
    local command
    for command in check doctor list ls show diff disable resolve; do
        assert_entry_parity "$command" --help
        assert_entry_parity "$command" -h
    done
    assert_entry_parity sync --help
    assert_entry_parity rollback --help
}

@test "parity: an unknown command is refused with the usage on stderr like Bash" {
    assert_entry_parity nonexistent
    assert_entry_parity HELP
    assert_entry_parity --bogus
    assert_entry_parity nonexistent --help
}
```

Run, outside the sandbox: `bats --tap -f 'parity: help|parity: --help after|parity: an unknown command' tests/native_parity.bats`
Expected: `not ok 1` with `exit status differs for []: bash=0 native=2`, `not ok 2` with `exit status differs for [check --help]: bash=0 native=2`, `not ok 3` with `exit status differs for [nonexistent]: bash=1 native=2`. The binary from Task 0 still answers these through clap; `assert_parity help` passes because the dispatcher does not delegate `help` yet.

- [x] **Step 2: Write the failing tests**

Create `src/cli/usage.rs` with its tests module, and add `pub mod usage;` to `src/cli/mod.rs` between `pub mod upgrade_config;` and `pub mod workspace;`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn the_usage_matches_print_usage() {
        let text = usage(&Style::plain());
        assert!(text.starts_with(&format!(
            "\n  AgentSync v{}\n  Sync AI agent instructions to every tool from one source.\n\n  USAGE\n    agentsync <command> [options]\n\n  COMMANDS\n    init           Create .ai/ structure in current project\n",
            engine_version()
        )));
        assert!(text.contains(
            "\n    upgrade-config Re-pin agentsync_version in agent_sync.yaml\n    release        Bump version, tag, and push (maintainer)\n    version        Print version\n    help           Show this message\n\n  SYNC OPTIONS\n    --only <tools>"
        ));
        assert!(text.contains("\n    eval \"$(agentsync shell-init zsh)\"   # add to ~/.zshrc\n"));
        assert!(text.ends_with(
            "    agentsync refresh --dry-run\n\n  DOCS\n    https://github.com/yelmuratoff/agent\n\n"
        ));
        assert_eq!(text.lines().count(), 85);
    }

    #[test]
    fn a_terminal_gets_the_cli_colors_escapes() {
        let text = usage(&Style::colored());
        assert!(text.starts_with(&format!(
            "\n\x1b[1m  AgentSync\x1b[0m v{}\n\x1b[2m  Sync AI agent instructions to every tool from one source.\x1b[0m\n\n  \x1b[32mUSAGE\x1b[0m\n",
            engine_version()
        )));
        assert!(text.contains("\n    \x1b[36mshell-init\x1b[0m     Print a shell hook"));
        assert!(text.ends_with("\x1b[2m    https://github.com/yelmuratoff/agent\x1b[0m\n\n"));
    }

    #[test]
    fn usage_is_wanted_like_main_in_the_dispatcher() {
        for args in [
            &[][..],
            &[""],
            &["help"],
            &["help", "sync"],
            &["--help"],
            &["-h", "--bogus"],
            &["check", "--help"],
            &["ls", "-h"],
            &["resolve", "--help", "claude"],
        ] {
            assert!(wants_usage(&words(args)), "{args:?}");
        }
        for args in [
            &["HELP"][..],
            &["sync", "--help"],
            &["rollback", "-h"],
            &["check", "--only", "x", "--help"],
            &["enable", "--help"],
            &["version", "--help"],
        ] {
            assert!(!wants_usage(&words(args)), "{args:?}");
        }
    }

    #[test]
    fn an_unknown_command_is_refused_with_the_usage_on_stderr() {
        let mut err = Vec::new();
        let status = unknown_command("nonexistent", &Style::plain(), &mut err).unwrap();
        assert_eq!(status, 1);
        let err = String::from_utf8(err).unwrap();
        assert_eq!(
            err,
            format!(
                "Error: Unknown command: nonexistent\n\n{}",
                usage(&Style::plain())
            )
        );
    }
}
```

Run: `cargo test cli::usage 2>&1 | grep -E '^error' | head -3`
Expected: ``error[E0425]: cannot find function `usage` in this scope``, ``error[E0433]: cannot find type `Style` in this scope``, ``error[E0425]: cannot find function `engine_version` in this scope``: the module body and its imports do not exist yet.

- [x] **Step 3: Write the implementation**

In `src/style.rs`, after `plain`:

```rust
    #[cfg(test)]
    pub(crate) const fn colored() -> Self {
        Self { enabled: true }
    }
```

Prepend to `src/cli/usage.rs`:

```rust
//! The surface `bin/agentsync.sh` answers itself: `print_usage` for `help`,
//! `--help`, `-h`, and no command; the `--help` interception for commands whose
//! own parser does not read it; and the unknown-command refusal.

use std::io::Write;

use super::customize::put;
use crate::style::Style;
use crate::{Error, engine_version};

const COMMANDS: [(&str, &str); 29] = [
    ("init", "Create .ai/ structure in current project"),
    ("sync", "Sync instructions to all enabled tools"),
    ("rollback", "Restore targets from the latest backup"),
    ("check", "Verify outputs are in sync with source"),
    ("list", "Show available tools and their status"),
    ("enable", "Opt in to one or more tools"),
    ("disable", "Opt out of one or more tools"),
    ("add", "Scaffold a rule, skill, command, or subagent"),
    ("customize", "Create a per-field override for a tool"),
    ("simplify", "Remove override fields that match the base"),
    (
        "migrate",
        "Print and copy a prompt for upgrading an existing config",
    ),
    ("show", "Show effective config for a tool"),
    ("diff", "Show user overrides vs base defaults"),
    (
        "resolve",
        "Interactively reconcile overrides with base values",
    ),
    ("doctor", "Validate setup and surface warnings"),
    (
        "dedupe",
        "Remove source files that duplicate a parent .ai/src/",
    ),
    (
        "adopt",
        "Promote a manual edit in a generated file back into .ai/src/",
    ),
    (
        "profile",
        "Manage config-home profiles (work/personal variants)",
    ),
    (
        "generate",
        "Print a prompt to auto-generate project-specific rules",
    ),
    (
        "setup-hooks",
        "Install git hooks for automatic sync (--pre-commit optional)",
    ),
    (
        "shell-init",
        "Print a shell hook that auto-syncs on directory change",
    ),
    ("export", "Bundle .ai/src/ into a shareable archive"),
    ("import", "Import config from GitHub, archive, or directory"),
    ("refresh", "Pull new template files into existing .ai/src/"),
    (
        "update",
        "Update AgentSync to the latest version, or pin one: update <version>",
    ),
    (
        "upgrade-config",
        "Re-pin agentsync_version in agent_sync.yaml",
    ),
    ("release", "Bump version, tag, and push (maintainer)"),
    ("version", "Print version"),
    ("help", "Show this message"),
];

const SYNC_OPTIONS: &str = "    --only <tools>    Sync only these tools (comma-separated)
    --skip <tools>    Skip these tools (comma-separated)
    --profile <name>  Sync personal tools plus the named config-home profile
    --dry-run         Preview changes without writing
    --force           Overwrite destination files even if they were edited manually
    --if-stale        Sync only when source changed since the last sync (else no-op)
    --workspace       Run sync in every .ai/ below cwd (bottom-up alphabetical)
";

const EXAMPLES: &str = "    agentsync init
    agentsync list
    agentsync enable claude cursor
    agentsync add rule testing
    agentsync add skill deploy
    agentsync customize cursor
    agentsync simplify
    agentsync simplify cursor --apply
    agentsync show cursor
    agentsync diff
    agentsync doctor
    agentsync resolve
    agentsync adopt .cursor/rules/core.mdc
    agentsync adopt --all
    agentsync profile add hub
    agentsync sync
    agentsync sync --only claude,cursor
    agentsync sync --profile hub
    agentsync sync --dry-run
    agentsync sync --if-stale
    agentsync rollback
    agentsync rollback --list
    agentsync check
    agentsync setup-hooks --pre-commit
    eval \"$(agentsync shell-init zsh)\"   # add to ~/.zshrc
    agentsync generate
    agentsync generate React + TypeScript + Next.js project with Prisma ORM
    agentsync migrate
    agentsync export
    agentsync import https://github.com/user/repo
    agentsync refresh
    agentsync refresh --only rules,skills
    agentsync refresh --dry-run
";

/// Commands whose own parser does not read `--help`; `main` in
/// `bin/agentsync.sh` prints the usage when their first argument asks for it.
const HELP_INTERCEPTED: [&str; 8] = [
    "check", "doctor", "list", "ls", "show", "diff", "disable", "resolve",
];

/// `print_usage`.
pub fn usage(style: &Style) -> String {
    let mut text = format!(
        "\n{} v{}\n{}\n\n  {}\n    agentsync <command> [options]\n\n  {}\n",
        style.bold("  AgentSync"),
        engine_version(),
        style.dim("  Sync AI agent instructions to every tool from one source."),
        style.green("USAGE"),
        style.green("COMMANDS"),
    );
    for (name, summary) in COMMANDS {
        text.push_str(&format!(
            "    {}{}{summary}\n",
            style.cyan(name),
            " ".repeat(15 - name.len())
        ));
    }
    text.push_str(&format!("\n  {}\n", style.green("SYNC OPTIONS")));
    text.push_str(SYNC_OPTIONS);
    text.push_str(&format!("\n  {}\n", style.green("EXAMPLES")));
    text.push_str(EXAMPLES);
    text.push_str(&format!(
        "\n  {}\n{}\n\n",
        style.green("DOCS"),
        style.dim("    https://github.com/yelmuratoff/agent")
    ));
    text
}

/// Whether `bin/agentsync.sh` answers `args` with the usage: no command, an
/// empty one (`${1:-help}`), `help`, `--help`, `-h`, or an intercepted command
/// whose next argument is `--help` or `-h`. Later arguments are ignored.
pub fn wants_usage(args: &[String]) -> bool {
    let command = args.first().map(String::as_str).unwrap_or("");
    if matches!(command, "" | "help" | "--help" | "-h") {
        return true;
    }
    HELP_INTERCEPTED.contains(&command)
        && matches!(args.get(1).map(String::as_str), Some("--help" | "-h"))
}

/// The `*)` arm of `main`: the refusal and the usage on stderr, status 1.
pub fn unknown_command(command: &str, style: &Style, err: &mut dyn Write) -> Result<u8, Error> {
    put(
        err,
        format!(
            "{}: Unknown command: {command}\n\n{}",
            style.red("Error"),
            usage(style)
        )
        .as_bytes(),
    )?;
    Ok(1)
}
```

In `src/main.rs`, right after `guard_engine_version()?;` in `run`:

```rust
    let words: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if cli::usage::wants_usage(&words) {
        return print_usage();
    }
```

right before `let cli = Cli::parse_from(…)`:

```rust
    let command = words.first().map(String::as_str).unwrap_or_default();
    if !matches!(
        command,
        "version" | "list" | "ls" | "check" | "sync" | "rollback"
    ) {
        return cli::usage::unknown_command(command, &Style::for_stdout(), &mut std::io::stderr());
    }
```

and before `print_version`:

```rust
fn print_usage() -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    out.write_all(cli::usage::usage(&Style::for_stdout()).as_bytes())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
}

```

In `bin/agentsync.sh:280` append `help --help -h`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor add export import generate gen shell-init setup-hooks help --help -h "
```

In `tests/native_dispatch.bats`, replace the test "native: an unported command never reaches the binary", whose `help` is now listed, with:

```bash
@test "native: an unlisted command never reaches the binary" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync nonexistent
    [ "$status" -eq 1 ]
    [[ "$output" == *"Unknown command: nonexistent"* ]]
    [[ "$output" != *"native:"* ]]
}

@test "native: help and a missing command reach the binary" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync help
    [ "$status" -eq 42 ]
    [[ "$output" == *"native:help"* ]]
    run run_agentsync
    [ "$status" -eq 42 ]
    [[ "$output" == "native:"$'\n'* ]]
}
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in cli native_dispatch; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
grep -c '^@test' tests/native_dispatch.bats
bats --tap -f 'parity: help|parity: --help after|parity: an unknown command' tests/native_parity.bats
```

Expected: `289 passed`, `0`, `11`, `1`; `bash=0 native=0` for both files; `9` cases; `ok 1`, `ok 2`, `ok 3`.

- [x] **Step 5: Prove the fixture bites, run the references, lint, commit**

Change `Unknown command: {command}` to `Unknown command {command}` in `src/cli/usage.rs`, rebuild, rerun `bats --tap -f 'parity: an unknown command' tests/native_parity.bats`: `not ok 1` with `stderr differs for [nonexistent]` and `< Error: Unknown command: nonexistent` in the diff; revert and rebuild.

Recreate the harnesses when the session scratchpad no longer holds `phase5a/`. Both take `<engine bash|direct> <repo root> <out file>`; `direct` calls the release binary with no dispatcher in front of it. The pty harness needs `script`, which the agent sandbox refuses.

`phase5a/entry_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: entry_reference.sh <engine bash|direct> <repo root> <out file>
# Runs every argument shape the dispatcher answers itself and records status,
# stdout, and stderr apart. `bash` runs bin/agentsync.sh with AGENTSYNC_NATIVE=0;
# `direct` calls the release binary with no dispatcher in front of it.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/entry_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK/p"
: > "$OUT"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH AGENTSYNC_ENGINE_VERSION NO_COLOR
engine() {
    if [[ "$MODE" == direct ]]; then
        "$REPO/target/release/agentsync" "$@"
    else
        AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO" bash "$REPO/bin/agentsync.sh" "$@"
    fi
}
case_() {
    local rc=0
    (cd "$WORK/p" && engine "$@" > "$WORK/out" 2> "$WORK/err" < /dev/null) || rc=$?
    {
        printf '### agentsync'
        printf ' [%s]' "$@"
        printf '\nrc=%s\n--- stdout\n' "$rc"
        cat "$WORK/out"
        printf -- '--- stderr\n'
        cat "$WORK/err"
    } >> "$OUT"
}
case_
case_ ""
case_ help
case_ help sync --bogus
case_ --help
case_ -h
case_ -h extra
case_ nonexistent
case_ HELP
case_ nonexistent --help
case_ --bogus
for c in check doctor list ls show diff disable resolve; do
    case_ "$c" --help
    case_ "$c" -h
done
case_ show cursor --help
case_ sync --help
case_ rollback --help
case_ version
```

`phase5a/entry_tty.sh`:

```bash
#!/usr/bin/env bash
# Usage: entry_tty.sh <engine bash|direct> <repo root> <out file>
# Runs `help` and an unknown command on a pseudo-terminal through `script` and
# keeps the escape codes, so the coloured usage can be compared byte for byte.
# AGENTSYNC_NO_UPDATE_CHECK keeps the dispatcher's terminal-only notices out.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/tty_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK/p"
: > "$OUT"
unset AGENTSYNC_REPO_ROOT NO_COLOR
export AGENTSYNC_NO_UPDATE_CHECK=1
if [[ "$MODE" == direct ]]; then
    CMD="$REPO/target/release/agentsync"
else
    CMD="AGENTSYNC_NATIVE=0 AGENTSYNC_HOME=$REPO bash $REPO/bin/agentsync.sh"
fi
scenario() {
    local name="$1" args="$2"
    local transcript="$WORK/$name.txt"
    (cd "$WORK/p" && script -q "$transcript" bash -c "$CMD $args; echo \"rc=\$?\"" < /dev/null > /dev/null 2>&1)
    {
        echo "### $name :: $args"
        tr -d '\r' < "$transcript" | od -c | tail -n +1 | head -400
    } >> "$OUT"
}
scenario "help" "help"
scenario "unknown" "nonexistent"
scenario "intercepted" "doctor --help"
```

```bash
bash phase5a/entry_reference.sh bash "$PWD" phase5a/entry_bash.out && bash phase5a/entry_reference.sh direct "$PWD" phase5a/entry_direct.out
wc -l < phase5a/entry_direct.out
grep -c '^### ' phase5a/entry_direct.out
diff phase5a/entry_bash.out phase5a/entry_direct.out | grep -c '^[<>]'
bash phase5a/entry_tty.sh bash "$PWD" phase5a/tty_bash.out && bash phase5a/entry_tty.sh direct "$PWD" phase5a/tty_direct.out
wc -l < phase5a/tty_direct.out
grep -c '033' phase5a/tty_direct.out
diff phase5a/tty_bash.out phase5a/tty_direct.out | grep -c '^[<>]'
```

Expected: `2468` lines over `31` shapes with `0` differing lines; `738` dump lines, `200` of them holding an escape, with `0` differing lines over the three pty scenarios (the usage, the refusal, and `doctor --help`). The line counts hold while `VERSION` keeps its width.

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/usage.rs src/style.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_dispatch.bats tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-5a-entry.md
git commit -m "feat(native): port help and the dispatcher's own answers"
```

---

### Task 2: Verify and Module Map

**Files:**
- Modify: `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Module map and outputs**

In the "Engine modules" block, after the `lib/setup_hooks.sh` row, add:

```text
bin/agentsync.sh print_usage, the --help interception, *) → src/cli/usage.rs   Phase 5a, ported
```

In "bats ownership", set `tests/native_parity.bats 65` to `68` and `tests/native_dispatch.bats 8` to `9`. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 2: Verify (outside the agent sandbox)**

Recreate `phase5a/native_suite.sh` when the scratchpad no longer holds it:

```bash
#!/usr/bin/env bash
# Usage: native_suite.sh <repo root> <mode 0|1|both> <out file>
# Runs every bats file one at a time under the given engine(s) and prints
# `<file> bash=<failures> native=<failures>` per file, then a total.
set -uo pipefail
REPO="$1"; WHICH="$2"; OUT="$3"
: > "$OUT"
cd "$REPO" || exit 1
total_bash=0; total_native=0
for f in tests/*.bats; do
    name="${f#tests/}"; name="${name%.bats}"
    b="-"; n="-"
    if [[ "$WHICH" == "0" || "$WHICH" == "both" ]]; then
        b=$(AGENTSYNC_NATIVE=0 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_bash=$((total_bash + b))
    fi
    if [[ "$WHICH" == "1" || "$WHICH" == "both" ]]; then
        n=$(AGENTSYNC_NATIVE=1 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_native=$((total_native + n))
    fi
    printf '%s bash=%s native=%s\n' "$name" "$b" "$n" >> "$OUT"
done
printf 'TOTAL bash=%s native=%s\n' "$total_bash" "$total_native" >> "$OUT"
```

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase5a/native_suite.sh "$PWD" both phase5a/suite_both.out && tail -1 phase5a/suite_both.out
```

Expected: `289 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 50 bats files, each run one at a time under both engines.

- [x] **Step 3: Commit**

```bash
git add .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-5a-entry.md
git commit -m "docs(native): map the phase 5a module"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the three parity fixtures pass, and a `## Completion receipt` records the fresh verification. `sync` and `check` do not change, so no timings are due. Phase 5 stays open until the plans for 5b, 5c, 5d, and 5e are closed as well.

## Completion receipt

### Decisions the review took

All four as recommended: the maintainer asked on 2026-09-16 to start 5a from this plan, and on 2026-09-17 to continue it.

### Global Constraints

| Constraint | Satisfied by |
|---|---|
| `help` and the refusal write nothing | `src/cli/usage.rs` builds strings and writes only to the writer `main` hands `unknown_command`; `print_usage` in `src/main.rs` writes stdout alone |
| The only Bash change is `_NATIVE_COMMANDS`; ShellCheck clean | `git diff --stat 1f20da0..HEAD -- lib bin` names only `bin/agentsync.sh`, one line changed at 280; ShellCheck exit 0 |
| Byte-for-byte parity through the dispatcher and with the binary called directly, except the accepted deviations | `tests/native_parity.bats`: `assert_entry_parity` and the three fixtures; `entry_reference.sh` and `entry_tty.sh` in Task 1 Step 5; `version --help` and `check --only x --help` stay under the Phase 1 clap deviation and outside the fixtures |
| `unsafe_code = "forbid"`, fmt and clippy clean, no new dependency; `main.rs` alone reads the environment and terminal state; nothing new spawned | `Cargo.toml` and `Cargo.lock` unchanged since `1f20da0`; `src/cli/usage.rs` reads no variable and spawns nothing, and `Style::for_stdout()` is called from `src/main.rs` |
| The terminal notices stay in the dispatcher | `check_for_updates` in `bin/agentsync.sh` `main` is untouched; `usage.rs` prints no notice |
| Expected values captured from Bash | `entry_reference.sh` (31 shapes, 2468 lines) and `entry_tty.sh` (3 scenarios, 738 lines), reproduced in Task 1 Step 5 |
| Conventional Commits, at most 72 characters, no trailers | `1f20da0`, `c991bd3`, `7d40d04`, `2d1b99c`, and the close commit |
| bats one file at a time | `native_suite.sh`, `suite_part.sh`, and `one_mode.sh` each run one file under one engine at a time |

### Fresh verification, 2026-09-17, macOS 26.5 arm64, outside the agent sandbox where noted

- `cargo test`: 289 passed (lib), 0 passed (doc), 11 passed (cli), 1 passed (interrupt).
- `cargo clippy --all-targets -- -D warnings`: exit 0. `cargo fmt --all --check`: exit 0.
- `shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh`: exit 0.
- bats, every file alone under both engines against the release binary of `c991bd3` (no code or test changed after it): 50 files, `TOTAL bash=0 native=0`. The run took three passes because macOS killed the first two for low memory (`fseventsd` held 18G on the 16 GB host): `native_suite.sh both` covered `add` through `native_dispatch` (28 files); `one_mode.sh` ran `native_parity` (68 cases) under Bash in 425 s and natively in 399 s; `suite_part.sh` covered the other 21 files after the maintainer freed memory.
- Wall times under that pressure are not a measurement: `rollback_preflight` took 2808 s, `profiles`, `resource_resolver`, and `shared` about 950 s each, `refresh` 627 s, `source_overrides` 572 s, while `sync` took 45 s. None of those commands changed in this slice.
- Mutation in Task 1 Step 5: `Unknown command: {command}` → `Unknown command {command}` failed the unknown-command fixture on stderr; reverted, rebuilt, byte-identical to the verified draft.
- `sync` and `check` are unchanged; no timings.

### Skipped, deferred, open

- **Windows**: native bats runs stay off Windows until 5e; `cargo test` still runs there.
- **`update` and `release`** are refused as unknown by the binary until 5b and 5d, per decision 2; the dispatcher still serves both.
- **Not pushed.** The branch is 5 commits ahead of `origin/feat/native-engine-phase-1`; the cutover (5e) is the first release point.

## Run log

### 2026-09-16 — Phase 5 sliced, 5a planned
- Commits: this plan, with the Phase 5 slices and decisions in the spec and the Phase 5 slice rule in `.ai/src/commands/native-next.md`. The four decisions the spec left open (downloads, clone installs, pre-binary pins, the dispatcher) were handed to the plan author and recorded as recommended; none of them changes this slice.
- Verified: the Rust in Task 1 was drafted in the tree and then parked in the session scratchpad: `cargo test` 289/0/11/1, fmt and clippy clean; `cli.bats` and `native_dispatch.bats` green under both engines; the three fixtures `ok` against the draft binary and `not ok` on the Step 5 mutation; `entry_reference.sh` 2468 lines over 31 shapes and `entry_tty.sh` 738 dump lines, both with 0 differing lines. Against the release binary of `02e4e01`, 12 of 14 probed shapes answered with a clap error and status 2, which Step 1 expects; `sync --help` and `rollback --help` already matched Bash. The Step 2 compile errors were captured by building the tests module alone. `yelmuratoff/agent` redirects to `yelmuratoff/agent_sync`, which has no GitHub release yet; cargo-dist's latest release is 0.33.0 (2026-09-11). No Bash bug turned up.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-16 — Task 0 and Task 1 done
- Commits: this commit, feat(native): port help and the dispatcher's own answers.
- Verified: baseline `cargo test` 285/0/11/1, and `cli`, `native_dispatch`, and `native_parity` (outside the sandbox) at 0 failures in Bash with 8, 8, and 65 cases; the three new fixtures `not ok` against the `1f20da0` binary with the three expected status lines; Step 2's three compile errors as listed; `cargo test` 289/0/11/1; `cli.bats` and `native_dispatch.bats` (9 cases) at `bash=0 native=0`; fixtures `ok 1` to `ok 3`, then `not ok 1` on the mutation with `stderr differs for [nonexistent]` and `ok 1` after the revert; `entry_reference.sh` 2468 lines over 31 shapes and `entry_tty.sh` 738 dump lines with 200 escapes, both with 0 differing lines; ShellCheck, fmt, and clippy exit 0.
- Plan amended: none.
- Next: Task 2 Step 1.
- Blocker: none.

### 2026-09-17 — Task 2 blocked
- Commits: this commit, docs(native): log run 2026-09-17. Step 1's edits to `.ai/src/skills/native-port/references/module-map.md` and `.ai/.sync-manifest` stay in the tree for Step 3's commit.
- Verified: Step 1 done (the `src/cli/usage.rs` row, 68 and 9 cases with `native_dispatch.bats` moved above the 8s; `sync --dry-run` showed no drift before `sync --force`, and `.claude/commands/native-next.md` carries the Phase 5 slice rule). Step 2: `cargo test` 289/0/11/1, clippy, fmt, and ShellCheck exit 0; `native_suite.sh both` covered 28 of the 50 files, `add` through `native_dispatch`, all at `bash=0 native=0`, then macOS killed it for low memory while `native_parity` ran; `native_parity` alone under both engines passed the 10-minute mark and was killed the same way.
- Plan amended: none.
- Next: Task 2 Step 2, for the 22 files not yet covered: native_parity opencode outputs_mode paths profiles refresh release resource_resolver rollback_preflight rollback shared shell_init simplify source_overrides sync_options sync team_workflow tmp update_snapshot update version_pin workspace.
- Blocker: memory on the 16 GB host. `top -o mem` showed `fseventsd` at 18G, then `java` 4897M and 1624M, WebKit 3443M; `memory_pressure` reported 27% free after the kill. No bats or agentsync process was left behind. The remaining files need the memory freed (restart `fseventsd` or reboot, stop the Java daemons) or a CI run of the branch.

### 2026-09-17 — phase closed
- Commits: `2d1b99c` docs(native): map the phase 5a module; this commit, docs(native): close phase 5a.
- Verified: after the maintainer freed memory, `native_parity` passed alone under Bash (68 cases, 425 s) and natively (399 s), and `suite_part.sh` ran the other 21 files at `bash=0 native=0`, so all 50 files are at `TOTAL bash=0 native=0`; fresh `cargo test` 289/0/11/1, and clippy, fmt, and ShellCheck exit 0. Several files ran for 10 to 47 minutes under memory pressure; see the receipt.
- Plan amended: none.
- Next: Phase 5b, `release` with the crate version: its plan, per the spec's Phase 5 section.
- Blocker: none.
