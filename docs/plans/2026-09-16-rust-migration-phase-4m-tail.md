# Rust Migration Phase 4m: Native `generate`, `shell-init`, and `setup-hooks`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port the last three standalone commands so the binary answers them byte for byte: `agentsync generate` like `cmd_generate` in `lib/helpers/generate.sh` (the raw prompt when piped or given a context, the two-choice menu and the multi-line description on a terminal, the decorations and clipboard tip on stderr), `agentsync shell-init` like `cmd_shell_init` in `lib/helpers/shell_init.sh` (the zsh and bash snippets, `$SHELL` detection, the log-voice refusals with status 2), and `agentsync setup-hooks` like `lib/setup_hooks.sh` (the hooks for each outputs mode, the marked block appended once, the `core.hooksPath` refusal, the git checks). With them `_NATIVE_COMMANDS` lists every command Phase 4 names, and the phase closes.

**Architecture:** `src/cli/generate.rs` embeds `lib/prompts/generate.md` with `include_str!` and reads the menu from stdin as Bash did; `src/cli/shell_init.rs` holds the snippets as constants and speaks its refusals through `Log::capturing`, a small addition to `log.rs`; `src/cli/setup_hooks.rs` runs `git rev-parse` three times as the script did and writes the hook bodies from constants. `main` hands `generate` both terminal states, a line reader, and the clipboard command found on `PATH`, `shell-init` the `$SHELL` value and `_use_colors`, and `setup-hooks` the root `cmd_engine` exported. The seam stays the CLI process boundary: `tests/generate.bats`, `tests/shell_init.bats`, and `tests/hooks.bats` under `AGENTSYNC_NATIVE=1`, three parity fixtures, a reference harness with git config isolated, and a pty harness for the menu.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `generate` and `shell-init` write nothing; `setup-hooks` writes only below the hooks directory git names.
- No binary ships to users; without a binary every command runs in Bash. No Bash change; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state. The binary spawns `git` for `setup-hooks` and nothing else new.
- Disk-touching unit tests are `#[cfg(unix)]`; the `setup-hooks` tests pin `core.hooksPath` in the repository they create so the developer's global config never reaches them.
- Every expected value was captured from Bash on 2026-09-16: `phase4m/tail_reference.sh` (the non-interactive branches, 1884 lines, git config isolated as `tests/test_helper.bash` isolates it) and `phase4m/generate_tty.sh` (4 menu scenarios on a pty through `script`). The scripts are reproduced in Task 1 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-16 under the maintainer's standing instruction to run Phase 4 to its close; each was checked against Bash before the call.

1. **One task for the three commands.** Each is under 250 lines of Rust with no shared helper, and the phase closes on their commit, so they land together as `feat(native): port generate, shell-init, and setup-hooks`. Alternative: three tasks with three commits; rejected as ceremony for three ports that share nothing.
2. **`help` stays in Bash.** The spec's Phase 4 exit says `_NATIVE_COMMANDS` lists every command; `help`, `--help`, and `-h` print the dispatcher's own usage, which the design leaves to Bash until the shim goes in Phase 5, and `update` and `release` are rewritten there. After this plan the list names every other command. Alternative: port `print_usage` now; rejected because Phase 5 rewrites the shim it belongs to.
3. **Quirks and deviations.** Record as known quirks 52–53: `generate` ends with status 1 and no message when stdin closes before the menu or the description is answered (`read` under `set -e`); `setup-hooks` refuses `--bogus --help` for the unknown option first, and a `--bogus` anywhere wins over `--help`. Record as accepted deviations `generate`'s clipboard tip, which names the first of `pbcopy`, `wl-copy`, `xclip`, `xsel` found on `PATH` by the binary as `command -v` found it, and the log-voice error lines of `shell-init`, which the binary colours from stdout as `_use_colors` did. **Recommended:** as listed.

## Module closure

```text
lib/helpers/generate.sh          4-100    cmd_generate (the menu, the description loop)
                                 102-145  _output_prompt (decorations, clipboard tip)
lib/prompts/generate.md                   embedded as generate::PROMPT
lib/helpers/shell_init.sh        7-27     _shell_init_usage
                                 30-46    _shell_init_common; 48-58 _shell_init_zsh; 60-77 _shell_init_bash
                                 79-105   cmd_shell_init
lib/helpers/logging.sh           53-60    log_error (Log::capturing)
lib/setup_hooks.sh               29-56    the option loop and usage
                                 58-98    the root, git, and core.hooksPath checks
                                 100-116  OUTPUTS_MODE
                                 118-181  emit_sync_body, emit_precommit_gate_body
                                 183-221  append_agentsync_block, install_hook, the mode switch
bin/agentsync.sh                 280      _NATIVE_COMMANDS; 377-379 the setup-hooks, shell-init, and generate|gen arms
```

Reused: `paths::{logical_root, normalize, parent, leaf}`, `yaml_subset::value`, `Log`, `style`, `cli::customize::put`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in generate shell_init hooks native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the plan's latest commit; `277 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses.

---

### Task 1: Port `generate`, `shell-init`, and `setup-hooks`

**Files:**
- Create: `src/cli/generate.rs`, `src/cli/shell_init.rs`, `src/cli/setup_hooks.rs`
- Modify: `src/log.rs`, `src/cli/mod.rs`, `src/main.rs`, `bin/agentsync.sh`
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/log.rs
impl Log { pub fn capturing(colors: bool) -> Self; }
// src/cli/generate.rs
pub const PROMPT: &str;
pub struct Env<'a> { pub stdin_tty: bool, pub stdout_tty: bool, pub clipboard: Option<String>, pub read_line: &'a mut dyn FnMut() -> Option<String> }
pub fn generate(args: &[String], style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
// src/cli/shell_init.rs
pub fn snippet(shell: &str) -> String;
pub fn shell_init(args: &[String], shell_env: Option<&str>, style: &Style, colors: bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
// src/cli/setup_hooks.rs
pub fn setup_hooks(args: &[String], root: &str, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
```

- [x] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── generate, shell-init, setup-hooks ────────────────────────────────────────
# Every call runs off a terminal: generate prints the raw prompt, and the hooks
# live under .git/, which the tree comparison prunes, so they are compared here.

@test "parity: generate prints the prompt with and without a context like Bash" {
    assert_parity generate
    assert_parity generate "Flutter app" with BLoC
    assert_parity gen ""
    assert_parity generate "React + Next.js"
}

@test "parity: shell-init prints the hooks and refuses unknown shells like Bash" {
    assert_parity shell-init zsh
    assert_parity shell-init bash extra
    assert_parity shell-init --help
    assert_parity shell-init -h
    assert_parity shell-init fish
    SHELL=/usr/bin/zsh assert_parity shell-init
    SHELL=/bin/bash assert_parity shell-init
    SHELL= assert_parity shell-init
    SHELL=zsh assert_parity shell-init
}

_assert_same_hooks() {
    local name left right
    for name in post-merge post-checkout pre-commit; do
        left="$BATS_TEST_TMPDIR/bash/.git/hooks/$name"
        right="$BATS_TEST_TMPDIR/native/.git/hooks/$name"
        if [[ -f "$left" ]] || [[ -f "$right" ]]; then
            cmp -s "$left" "$right" || { echo "hook differs: $name" >&2; return 1; }
            [[ -x "$left" ]] && [[ -x "$right" ]]
        fi
    done
}

@test "parity: setup-hooks installs the hooks for each outputs mode like Bash" {
    assert_tree_parity setup-hooks --help
    assert_tree_parity setup-hooks --bogus --help
    assert_tree_parity setup-hooks
    _assert_same_hooks
    printf '#!/bin/sh\necho "existing hook"\n' > .git/hooks/post-merge
    chmod +x .git/hooks/post-merge
    assert_tree_parity setup-hooks --pre-commit
    _assert_same_hooks
    printf 'gitignore:\n  update: false\n' > .ai/agent_sync.yaml
    assert_tree_parity setup-hooks
    _assert_same_hooks
    mkdir -p .githooks
    git config core.hooksPath .githooks
    assert_tree_parity setup-hooks
    [ ! -e "$BATS_TEST_TMPDIR/native/.githooks/pre-commit" ]
    rm -rf .git
    assert_tree_parity setup-hooks
}
```

Run, outside the sandbox: `bats --tap -f 'parity: generate|parity: shell-init|parity: setup-hooks' tests/native_parity.bats`
Expected: `ok 1`, `ok 2`, `ok 3` (the native side still runs Bash until Step 3 lists the commands).

- [x] **Step 2: Write the failing tests**

Create the three files with their tests modules, and add `pub mod generate;`, `pub mod setup_hooks;`, and `pub mod shell_init;` to `src/cli/mod.rs` in alphabetical order:

`src/cli/generate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        args: &[&str],
        stdin_tty: bool,
        stdout_tty: bool,
        clipboard: Option<&str>,
        lines: &[&str],
    ) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut queue: std::collections::VecDeque<String> =
            lines.iter().map(|l| l.to_string()).collect();
        let mut read_line = move || queue.pop_front();
        let mut env = Env {
            stdin_tty,
            stdout_tty,
            clipboard: clipboard.map(str::to_string),
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = generate(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn prompt() -> String {
        format!("{}\n", PROMPT.trim_end_matches('\n'))
    }

    #[test]
    fn a_piped_run_prints_the_raw_prompt_like_cmd_generate() {
        let (status, out, err) = run(&[], false, false, Some("pbcopy"), &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(out, prompt());
        let (status, out, err) = run(&["Flutter app", "with BLoC"], true, false, None, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "## My Project\n\nFlutter app with BLoC\n\n---\n\n{}",
                prompt()
            )
        );
    }

    #[test]
    fn a_terminal_gets_the_decorations_and_the_clipboard_tip() {
        let (status, out, err) = run(&["React"], false, true, Some("pbcopy"), &[]);
        assert_eq!(status, 0);
        assert!(out.starts_with("## My Project\n\nReact\n\n---\n\n"));
        assert_eq!(
            err,
            "\n  ─── prompt below ───────────────────────────────────────────\n\n\n  ─── end of prompt ──────────────────────────────────────────\n\n  Tip: run agentsync generate | pbcopy to copy to clipboard.\n\n"
        );
        let (_, _, err) = run(&["React"], false, true, None, &[]);
        assert!(!err.contains("Tip: run"));
    }

    #[test]
    fn the_menu_reads_a_choice_and_a_description_like_cmd_generate() {
        let (status, out, err) = run(&[], true, true, None, &[""]);
        assert_eq!(status, 0);
        assert_eq!(out, prompt());
        assert!(err.starts_with(
            "\n  AgentSync Generate\n\n  Choose what to generate:\n\n    1) Base prompt only\n"
        ));
        assert!(err.contains("  ▸ Choice [1/2]: \n\n  ─── prompt below"));
        let (status, out, err) = run(
            &[],
            true,
            true,
            None,
            &["x", "2", "Rust CLI", "", "with clap", "", ""],
        );
        assert_eq!(status, 0);
        assert!(out.starts_with("## My Project\n\nRust CLI\n\nwith clap\n\n---\n\n"));
        assert_eq!(err.matches("Choice [1/2]:").count(), 2);
        assert!(err.contains("  ╭─────"));
        assert!(err.contains("  │   │   │   │   │ \n\n  ─── prompt below"));
        let (status, out, _) = run(&[], true, true, None, &["2", "only line"]);
        assert_eq!((status, out.as_str()), (1, ""));
        let (status, _, _) = run(&[], true, true, None, &[]);
        assert_eq!(status, 1);
    }
}
```

`src/cli/shell_init.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], shell: Option<&str>, colors: bool) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = shell_init(&args, shell, &Style::plain(), colors, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn the_snippets_carry_the_markers_and_the_hook_like_shell_init() {
        let (status, zsh, err) = run(&["zsh", "extra"], None, false);
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(zsh.starts_with("# >>> agentsync shell hook (zsh) >>>\n_agentsync_autosync() {\n"));
        assert!(zsh.contains("\n  add-zsh-hook chpwd _agentsync_autosync\n"));
        assert!(
            zsh.contains("\n    AGENTSYNC_REPO_ROOT=\"$PWD\" agentsync sync --if-stale || true\n")
        );
        assert!(zsh.ends_with("_agentsync_autosync\n# <<< agentsync shell hook (zsh) <<<\n"));
        assert!(!zsh.contains("cd "));
        let (_, bash, _) = run(&["bash"], None, false);
        assert!(bash.starts_with("# >>> agentsync shell hook (bash) >>>\n"));
        assert!(bash.contains(
            "PROMPT_COMMAND=\"_agentsync_prompt_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}\""
        ));
        assert!(bash.ends_with(
            "_AGENTSYNC_LAST_PWD=$PWD\n_agentsync_autosync\n# <<< agentsync shell hook (bash) <<<\n"
        ));
        assert_eq!(run(&[], Some("/usr/bin/zsh"), false).1, zsh);
        assert_eq!(run(&[], Some("/bin/bash"), false).1, bash);
        let (status, out, _) = run(&["--help"], None, false);
        assert_eq!((status, out.as_str()), (0, USAGE));
    }

    #[test]
    fn an_unknown_or_undetected_shell_exits_2_with_the_log_voice() {
        let (status, out, err) = run(&["fish"], None, false);
        assert_eq!((status, out.as_str()), (2, ""));
        assert_eq!(
            err,
            "[ERROR] Unsupported shell: fish (expected 'zsh' or 'bash')\n"
        );
        for shell in [None, Some(""), Some("zsh"), Some("/usr/local/bin/fish")] {
            let (status, out, err) = run(&[], shell, false);
            assert_eq!((status, out.as_str()), (2, ""));
            assert_eq!(
                err,
                "[ERROR] Could not detect your shell from $SHELL.\n  Pass one explicitly: agentsync shell-init zsh or agentsync shell-init bash\n"
            );
        }
        let (_, _, err) = run(&["fish"], None, true);
        assert_eq!(
            err,
            "\x1b[0;31m❌ [ERROR]\x1b[0m Unsupported shell: fish (expected 'zsh' or 'bash')\n"
        );
    }
}
```

`src/cli/setup_hooks.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = setup_hooks(&args, root, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// A repository whose hooks path is pinned to `.git/hooks`, so the
    /// developer's global `core.hooksPath` never reaches the test.
    fn repo(config: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            Command::new("git")
                .args(["-C", &root, "init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["-C", &root, "config", "core.hooksPath", ".git/hooks"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), config).unwrap();
        (dir, root)
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn local_outputs_get_the_sync_hooks_like_setup_hooks_sh() {
        let (dir, root) = repo("outputs: local\n");
        let (status, out, err) = run(&root, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "Configured post-merge hook.\nConfigured post-checkout hook.\nGit hooks configured for local outputs.\n"
        );
        let hook = dir.path().join(".git/hooks/post-merge");
        assert_eq!(mode(&hook), 0o755);
        assert_eq!(
            std::fs::read_to_string(&hook).unwrap(),
            format!(
                "#!/bin/sh\n\n\n{BLOCK_START}\n{}\n{BLOCK_END}\n",
                sync_body("sync")
            )
        );
        assert!(!dir.path().join(".git/hooks/pre-commit").exists());
        let (status, out, _) = run(&root, &["--pre-commit"]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "AgentSync hook already present in post-merge.\nConfigured post-merge hook.\nAgentSync hook already present in post-checkout.\nConfigured post-checkout hook.\nConfigured pre-commit hook.\nGit hooks configured for local outputs.\n"
        );
        let pre_commit = std::fs::read_to_string(dir.path().join(".git/hooks/pre-commit")).unwrap();
        assert!(pre_commit.contains("\n    agentsync sync --if-stale || echo"));
        std::fs::write(&hook, "#!/bin/sh\necho \"existing hook\"\n").unwrap();
        run(&root, &[]);
        assert!(
            std::fs::read_to_string(&hook)
                .unwrap()
                .starts_with("#!/bin/sh\necho \"existing hook\"\n\n# >>> AGENTSYNC")
        );
    }

    #[test]
    fn committed_outputs_get_the_gate_like_setup_hooks_sh() {
        let (dir, root) = repo("gitignore:\n  update: false\n");
        let (status, out, _) = run(&root, &[]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "Configured pre-commit hook.\nGit hooks configured for committed outputs.\n"
        );
        let gate = std::fs::read_to_string(dir.path().join(".git/hooks/pre-commit")).unwrap();
        assert_eq!(
            gate,
            format!("#!/bin/sh\n\n\n{BLOCK_START}\n{GATE_BODY}\n{BLOCK_END}\n")
        );
        assert!(!dir.path().join(".git/hooks/post-merge").exists());
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "outputs: \"committed\"\n",
        )
        .unwrap();
        assert_eq!(outputs_mode(&root), "committed");
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "outputs: other\n").unwrap();
        assert_eq!(outputs_mode(&root), "local");
    }

    #[test]
    fn another_hooks_path_a_missing_repo_and_bad_options_are_refused_like_setup_hooks_sh() {
        let (dir, root) = repo("outputs: local\n");
        std::fs::create_dir_all(dir.path().join(".githooks")).unwrap();
        assert!(
            Command::new("git")
                .args(["-C", &root, "config", "core.hooksPath", ".githooks"])
                .status()
                .unwrap()
                .success()
        );
        let (status, out, err) = run(&root, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "This repository points core.hooksPath at another directory, so AgentSync\nwill not write there:\n\n  {root}/.githooks\n\nA hook manager (husky, lefthook, pre-commit) most likely owns it. Add this\nto the hook it manages instead:\n\n  command -v agentsync >/dev/null 2>&1 && agentsync sync --if-stale || true\n\n"
            )
        );
        assert!(!dir.path().join(".githooks/post-merge").exists());
        let (status, _, err) = run(&root, &["--bogus", "--help"]);
        assert_eq!(status, 2);
        assert_eq!(
            err,
            "Error: Unknown option: --bogus\nUsage: agentsync setup-hooks [--pre-commit]\n"
        );
        let (status, out, _) = run(&root, &["--help"]);
        assert_eq!((status, out.as_str()), (0, HELP));
        let missing = format!("{root}/nowhere");
        let (status, _, err) = run(&missing, &[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            format!("Error: Repository root not found: {missing}\n")
        );
        let plain = tempfile::tempdir().unwrap();
        let plain_root = std::fs::canonicalize(plain.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (status, _, err) = run(&plain_root, &[]);
        assert_eq!(status, 1);
        assert_eq!(err, format!("Error: Not a git repository: {plain_root}\n"));
    }
}
```

Run: `cargo test cli:: 2>&1 | grep -E '^error' | head -3`
Expected: compile errors naming `generate`, `shell_init`, and `setup_hooks`.

- [x] **Step 3: Write the implementation**

In `src/log.rs`, before `streaming`:

```rust
    /// A log that keeps its lines for `lines()`, coloured when `colors`.
    pub fn capturing(colors: bool) -> Self {
        Self {
            lines: Vec::new(),
            sink: None,
            colors,
        }
    }
```

Prepend to `src/cli/generate.rs`:

```rust
//! `agentsync generate`: `cmd_generate` of `lib/helpers/generate.sh`, which
//! prints the shipped prompt with an optional project description, and asks
//! for one on a terminal.

use std::io::Write;

use super::customize::put;
use crate::Error;
use crate::style::Style;

/// `lib/prompts/generate.md`, embedded.
pub const PROMPT: &str = include_str!("../../lib/prompts/generate.md");

/// What `generate` takes from the process.
pub struct Env<'a> {
    /// `-t 0` and `-t 1`: the menu opens only when both are terminals.
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    /// The first of `pbcopy`, `wl-copy`, `xclip -selection clipboard`,
    /// `xsel --clipboard --input` on `PATH`, for the tip.
    pub clipboard: Option<String>,
    /// `read -r` on stdin: the line without its newline, `None` at end of input.
    pub read_line: &'a mut dyn FnMut() -> Option<String>,
}

/// `_output_prompt`.
fn output_prompt(
    context: &str,
    style: &Style,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Error> {
    let mut text = String::new();
    if !context.is_empty() {
        text.push_str("## My Project\n\n");
        text.push_str(context);
        text.push_str("\n\n---\n\n");
    }
    text.push_str(PROMPT.trim_end_matches('\n'));
    text.push('\n');
    if !env.stdout_tty {
        return put(out, text.as_bytes());
    }
    put(
        err,
        format!(
            "\n  {}\n\n",
            style.dim("─── prompt below ───────────────────────────────────────────")
        )
        .as_bytes(),
    )?;
    put(out, text.as_bytes())?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    put(
        err,
        format!(
            "\n  {}\n\n",
            style.dim("─── end of prompt ──────────────────────────────────────────")
        )
        .as_bytes(),
    )?;
    if let Some(clipboard) = &env.clipboard {
        put(
            err,
            format!(
                "  {} {} {}\n\n",
                style.dim("Tip: run"),
                style.cyan(&format!("agentsync generate | {clipboard}")),
                style.dim("to copy to clipboard.")
            )
            .as_bytes(),
        )?;
    }
    Ok(())
}

/// `cmd_generate`: the prompt on `out`, the conversation on `err`. A closed
/// stdin ends the run with status 1, as `read` under `set -e` did.
pub fn generate(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let context = args.join(" ");
    if !context.is_empty() {
        output_prompt(&context, style, env, out, err)?;
        return Ok(0);
    }
    if !env.stdin_tty || !env.stdout_tty {
        output_prompt("", style, env, out, err)?;
        return Ok(0);
    }
    put(
        err,
        format!(
            "\n{}\n\n  Choose what to generate:\n\n    {} Base prompt only\n       Ready-to-paste prompt without project details.\n\n    {} Prompt + project description\n       You describe your stack, and it gets included in the prompt.\n\n",
            style.bold("  AgentSync Generate"),
            style.cyan("1)"),
            style.cyan("2)")
        )
        .as_bytes(),
    )?;
    let mut choice = String::new();
    while choice != "1" && choice != "2" {
        put(
            err,
            format!("  {} Choice [1/2]: ", style.green("▸")).as_bytes(),
        )?;
        err.flush().map_err(|e| Error::io("<stderr>", e))?;
        let Some(line) = (env.read_line)() else {
            return Ok(1);
        };
        choice = if line.is_empty() {
            "1".to_string()
        } else {
            line
        };
    }
    if choice == "1" {
        put(err, b"\n")?;
        output_prompt("", style, env, out, err)?;
        return Ok(0);
    }
    put(
        err,
        format!(
            "\n  ╭─────────────────────────────────────────────────────────╮\n  │  Describe your project: stack, frameworks, conventions  │\n  │  Type as many lines as you want.                        │\n  │  Press {} twice (empty line) when done.              │\n  ╰─────────────────────────────────────────────────────────╯\n\n",
            style.cyan("Enter")
        )
        .as_bytes(),
    )?;
    let mut lines = String::new();
    let mut prev_empty = false;
    loop {
        put(err, format!("  {} ", style.dim("│")).as_bytes())?;
        err.flush().map_err(|e| Error::io("<stderr>", e))?;
        let Some(line) = (env.read_line)() else {
            return Ok(1);
        };
        if line.is_empty() {
            if prev_empty {
                break;
            }
            prev_empty = true;
            lines.push('\n');
        } else {
            prev_empty = false;
            if !lines.is_empty() {
                lines.push('\n');
            }
            lines.push_str(&line);
        }
    }
    let lines = lines.trim_end_matches('\n');
    put(err, b"\n")?;
    output_prompt(lines, style, env, out, err)?;
    Ok(0)
}
```

Prepend to `src/cli/shell_init.rs`:

```rust
//! `agentsync shell-init`: `cmd_shell_init` of `lib/helpers/shell_init.sh`,
//! which prints the shell hook that runs `sync --if-stale` on entering a
//! project. Stdout carries nothing but the snippet, so `>> ~/.zshrc` stays clean.

use std::io::Write;

use super::customize::put;
use crate::Error;
use crate::log::Log;
use crate::style::Style;

const USAGE: &str = "Usage: agentsync shell-init [zsh|bash]

  Prints a shell snippet that runs 'agentsync sync --if-stale' for the
  current .ai/ project when you enter its root directory, so generated
  outputs stay fresh without syncing parent projects from descendants.

  Recommended: add one of these to your rc file. Eval'ing it regenerates
  the hook each session, so upgrades and fixes apply without re-editing:

    eval \"$(agentsync shell-init zsh)\"      # in ~/.zshrc
    eval \"$(agentsync shell-init bash)\"     # in ~/.bashrc

  Or freeze a copy with 'agentsync shell-init zsh >> ~/.zshrc', but then
  re-run it after each upgrade to pick up changes.

  The shell is auto-detected from $SHELL when omitted.
  Set AGENTSYNC_NO_AUTO_SYNC=1 to disable without removing the snippet.
";

const COMMON: &str = "_agentsync_autosync() {
  # Never `cd` here: as a zsh chpwd hook this fires on every directory change,
  # so a `cd` would re-trigger it and recurse (FUNCNEST blow-up). Point the sync
  # at the project via AGENTSYNC_REPO_ROOT instead, and guard against re-entry.
  [ -n \"${_AGENTSYNC_BUSY:-}\" ] && return 0
  [ -n \"${AGENTSYNC_NO_AUTO_SYNC:-}\" ] && return 0
  command -v agentsync >/dev/null 2>&1 || return 0
  _AGENTSYNC_BUSY=1
  if [ -d \"$PWD/.ai/src\" ]; then
    AGENTSYNC_REPO_ROOT=\"$PWD\" agentsync sync --if-stale || true
  fi
  unset _AGENTSYNC_BUSY
}
";

const ZSH: &str = "autoload -Uz add-zsh-hook 2>/dev/null
if (( ${+functions[add-zsh-hook]} )); then
  add-zsh-hook chpwd _agentsync_autosync
fi
_agentsync_autosync
# <<< agentsync shell hook (zsh) <<<
";

const BASH: &str = "_agentsync_prompt_hook() {
  if [ \"$PWD\" != \"${_AGENTSYNC_LAST_PWD:-}\" ]; then
    _AGENTSYNC_LAST_PWD=$PWD
    _agentsync_autosync
  fi
}
case \"${PROMPT_COMMAND:-}\" in
  *_agentsync_prompt_hook*) : ;;
  *) PROMPT_COMMAND=\"_agentsync_prompt_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}\" ;;
esac
_AGENTSYNC_LAST_PWD=$PWD
_agentsync_autosync
# <<< agentsync shell hook (bash) <<<
";

/// The snippet for `zsh` or `bash`.
pub fn snippet(shell: &str) -> String {
    let tail = if shell == "zsh" { ZSH } else { BASH };
    format!("# >>> agentsync shell hook ({shell}) >>>\n{COMMON}{tail}")
}

/// `cmd_shell_init`: `shell_env` is `$SHELL`, `colors` is `_use_colors` for
/// the log line; only the first argument is read, as Bash read it.
pub fn shell_init(
    args: &[String],
    shell_env: Option<&str>,
    style: &Style,
    colors: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut shell = args.first().cloned().unwrap_or_default();
    if shell == "--help" || shell == "-h" {
        return put(out, USAGE.as_bytes()).map(|()| 0);
    }
    if shell.is_empty() {
        match shell_env.unwrap_or("") {
            s if s.ends_with("/zsh") => shell = "zsh".to_string(),
            s if s.ends_with("/bash") => shell = "bash".to_string(),
            _ => {}
        }
    }
    let error = |message: &str| -> String {
        let mut log = Log::capturing(colors);
        log.error(message);
        log.lines()
            .iter()
            .map(|(_, line)| format!("{line}\n"))
            .collect()
    };
    match shell.as_str() {
        "zsh" | "bash" => put(out, snippet(&shell).as_bytes()).map(|()| 0),
        "" => {
            put(
                err,
                error("Could not detect your shell from $SHELL.").as_bytes(),
            )?;
            put(
                err,
                format!(
                    "  Pass one explicitly: {} or {}\n",
                    style.cyan("agentsync shell-init zsh"),
                    style.cyan("agentsync shell-init bash")
                )
                .as_bytes(),
            )?;
            Ok(2)
        }
        other => {
            put(
                err,
                error(&format!(
                    "Unsupported shell: {other} (expected 'zsh' or 'bash')"
                ))
                .as_bytes(),
            )?;
            Ok(2)
        }
    }
}
```

Prepend to `src/cli/setup_hooks.rs`:

```rust
//! `agentsync setup-hooks`: `lib/setup_hooks.sh`, which installs the git hooks
//! that suit the project's outputs mode, appending one marked block per hook
//! and leaving whatever the hook already ran.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::customize::put;
use crate::{Error, yaml_subset};

const USAGE: &str = "Usage: agentsync setup-hooks [--pre-commit]";
const BLOCK_START: &str = "# >>> AGENTSYNC AUTO SYNC START >>>";
const BLOCK_END: &str = "# <<< AGENTSYNC AUTO SYNC END <<<";

const HELP: &str = "Usage: agentsync setup-hooks [--pre-commit]

  Installs the git hooks that suit this project's outputs mode.

  committed: pre-commit re-syncs and fails the commit when a
             generated file changed, so outputs never lag source.
  local:     post-merge and post-checkout run 'agentsync sync'
             after pull/checkout.

  --pre-commit   In local mode, also install a pre-commit hook
                 that runs 'agentsync sync --if-stale'.

  Set AGENTSYNC_SKIP_HOOKS=1 to make the installed hooks no-ops.
";

const HOOKS_PATH_ELSEWHERE: &str =
    "This repository points core.hooksPath at another directory, so AgentSync
will not write there:

  {hooks}

A hook manager (husky, lefthook, pre-commit) most likely owns it. Add this
to the hook it manages instead:

  command -v agentsync >/dev/null 2>&1 && agentsync sync --if-stale || true

";

/// `emit_sync_body`: the POSIX body a hook runs, `$(...)` having dropped the
/// trailing newline.
fn sync_body(sync_args: &str) -> String {
    format!(
        "[ -n \"${{AGENTSYNC_SKIP_HOOKS:-}}\" ] && exit 0
if command -v agentsync >/dev/null 2>&1; then
    echo \"AgentSync: syncing AI config...\"
    agentsync {sync_args} || echo \"AgentSync: sync skipped — run 'agentsync sync' to see why.\" >&2
elif [ -f \"lib/sync.sh\" ]; then
    bash lib/sync.sh || true
elif [ -f \"agent/lib/sync.sh\" ]; then
    bash agent/lib/sync.sh || true
fi"
    )
}

/// `emit_precommit_gate_body`.
const GATE_BODY: &str = "[ -n \"${AGENTSYNC_SKIP_HOOKS:-}\" ] && exit 0
if command -v agentsync >/dev/null 2>&1; then
    agentsync sync --if-stale || echo \"AgentSync: sync skipped — run 'agentsync sync' to see why.\" >&2
fi
if [ -f \".ai/.sync-manifest\" ]; then
    # Flag a generated file only when the commit would leave it behind: the
    # worktree differs from the index (second status column) or it is untracked.
    # Already-staged output is exactly what belongs in the commit. The manifest
    # is awk's first input, so the hook needs no temp file.
    _as_dirty=$(git status --porcelain --untracked-files=all \\
        | awk -F'\\t' '
            NR == FNR { keep[$1] = 1; next }
            {
                code = substr($0, 1, 2)
                path = substr($0, 4)
                if ((code == \"??\" || substr(code, 2, 1) != \" \") && (path in keep))
                    print path
            }' \".ai/.sync-manifest\" - || true)
    if [ -n \"$_as_dirty\" ]; then
        echo \"AgentSync: generated files are out of date in this commit:\" >&2
        printf '%s\\n' \"$_as_dirty\" | sed 's/^/      /' >&2
        echo \"\" >&2
        echo \"  They are regenerated from .ai/src/ and belong in the same commit.\" >&2
        echo \"  Stage them (git add -A) and commit again.\" >&2
        echo \"\" >&2
        echo \"  To commit without them: AGENTSYNC_SKIP_HOOKS=1 git commit ...\" >&2
        exit 1
    fi
fi";

fn git(root: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

/// `_physical_path`: the parent resolved, the leaf as given.
fn physical(path: &str) -> String {
    let leaf = crate::paths::leaf(path);
    match std::fs::canonicalize(crate::paths::parent(path)) {
        Ok(parent) => format!("{}/{leaf}", parent.to_string_lossy()),
        Err(_) => path.to_string(),
    }
}

/// `OUTPUTS_MODE` from the first config present: `outputs`, else `committed`
/// when `gitignore.update` is `false`, else `local`.
fn outputs_mode(root: &str) -> &'static str {
    for rel in [".ai/agent_sync.yaml", "agent_sync.yaml"] {
        let path = Path::new(root).join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let mode = yaml_subset::value(&text, "outputs").replace('"', "");
        return match mode.as_str() {
            "committed" => "committed",
            "local" => "local",
            "" if yaml_subset::value(&text, "gitignore.update") == "false" => "committed",
            _ => "local",
        };
    }
    "local"
}

/// `install_hook`.
fn install_hook(
    hooks_dir: &Path,
    name: &str,
    body: &str,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let hook = hooks_dir.join(name);
    if !hook.is_file() {
        std::fs::write(&hook, "#!/bin/sh\n\n").map_err(|e| Error::io(&hook, e))?;
    }
    let existing = std::fs::read(&hook).map_err(|e| Error::io(&hook, e))?;
    let present = existing
        .windows(BLOCK_START.len())
        .any(|w| w == BLOCK_START.as_bytes());
    if present {
        put(
            out,
            format!("AgentSync hook already present in {name}.\n").as_bytes(),
        )?;
    } else {
        let mut appended = existing;
        appended.extend_from_slice(format!("\n{BLOCK_START}\n{body}\n{BLOCK_END}\n").as_bytes());
        std::fs::write(&hook, appended).map_err(|e| Error::io(&hook, e))?;
    }
    executable(&hook)?;
    put(out, format!("Configured {name} hook.\n").as_bytes())
}

/// `chmod +x`.
#[cfg(unix)]
fn executable(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    let mut perms = meta.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms).map_err(|e| Error::io(path, e))
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> Result<(), Error> {
    Ok(())
}

/// `lib/setup_hooks.sh`: `root` is `AGENTSYNC_REPO_ROOT` or the working
/// directory, as `cmd_engine` exported it.
pub fn setup_hooks(
    args: &[String],
    root: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut pre_commit = false;
    for arg in args {
        match arg.as_str() {
            "--pre-commit" => pre_commit = true,
            "--help" | "-h" => {
                put(out, HELP.as_bytes())?;
                return Ok(0);
            }
            other => {
                put(
                    err,
                    format!("Error: Unknown option: {other}\n{USAGE}\n").as_bytes(),
                )?;
                return Ok(2);
            }
        }
    }
    if !Path::new(root).is_dir() {
        put(
            err,
            format!("Error: Repository root not found: {root}\n").as_bytes(),
        )?;
        return Ok(1);
    }
    let root = crate::paths::normalize(root);
    if git(&root, &["rev-parse", "--git-dir"]).is_none() {
        put(
            err,
            format!("Error: Not a git repository: {root}\n").as_bytes(),
        )?;
        return Ok(1);
    }
    let mut hooks_dir = git(&root, &["rev-parse", "--git-path", "hooks"]).unwrap_or_default();
    if !hooks_dir.starts_with('/') {
        hooks_dir = format!("{root}/{hooks_dir}");
    }
    let git_dir = git(&root, &["rev-parse", "--absolute-git-dir"]).unwrap_or_default();
    if physical(&hooks_dir) != physical(&format!("{git_dir}/hooks")) {
        put(
            out,
            HOOKS_PATH_ELSEWHERE
                .replace("{hooks}", &hooks_dir)
                .as_bytes(),
        )?;
        return Ok(0);
    }
    let hooks_dir = PathBuf::from(hooks_dir);
    if outputs_mode(&root) == "committed" {
        install_hook(&hooks_dir, "pre-commit", GATE_BODY, out)?;
        put(out, b"Git hooks configured for committed outputs.\n")?;
    } else {
        install_hook(&hooks_dir, "post-merge", &sync_body("sync"), out)?;
        install_hook(&hooks_dir, "post-checkout", &sync_body("sync"), out)?;
        if pre_commit {
            install_hook(&hooks_dir, "pre-commit", &sync_body("sync --if-stale"), out)?;
        }
        put(out, b"Git hooks configured for local outputs.\n")?;
    }
    Ok(0)
}
```

In `src/main.rs`, before the `export`/`import` block:

```rust
    if matches!(
        args.first().and_then(|a| a.to_str()),
        Some("generate" | "gen")
    ) {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let mut read_line = || {
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
            }
        };
        let mut env = cli::generate::Env {
            stdin_tty: std::io::stdin().is_terminal(),
            stdout_tty: std::io::stdout().is_terminal(),
            clipboard: clipboard_command(),
            read_line: &mut read_line,
        };
        return cli::generate::generate(
            &rest,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("shell-init") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        return cli::shell_init::shell_init(
            &rest,
            var("SHELL").as_deref(),
            &Style::for_stdout(),
            log_colors(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
    if args.first().and_then(|a| a.to_str()) == Some("setup-hooks") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let env_root = var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty());
        let root = match env_root {
            Some(root) => root,
            None => paths::logical_root(None, &cwd, var("PWD").as_deref()),
        };
        return cli::setup_hooks::setup_hooks(
            &rest,
            &root,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

and before `log_colors`:

```rust
/// The clipboard command `_output_prompt` names in its tip: the first of
/// `pbcopy`, `wl-copy`, `xclip`, `xsel` on `PATH`, with the flags Bash printed.
fn clipboard_command() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let on_path = |name: &str| std::env::split_paths(&path).any(|dir| dir.join(name).is_file());
    [
        ("pbcopy", "pbcopy"),
        ("wl-copy", "wl-copy"),
        ("xclip", "xclip -selection clipboard"),
        ("xsel", "xsel --clipboard --input"),
    ]
    .iter()
    .find(|(name, _)| on_path(name))
    .map(|(_, command)| command.to_string())
}

```

In `bin/agentsync.sh:280` append `generate gen shell-init setup-hooks`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor add export import generate gen shell-init setup-hooks "
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in generate shell_init hooks; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'parity: generate|parity: shell-init|parity: setup-hooks' tests/native_parity.bats
```

Expected: `285 passed`, `0`, `11`, `1`; `0` for every file; `ok 1`, `ok 2`, `ok 3`.

- [x] **Step 5: Prove the fixture bites, run the references, lint, commit**

Change `Configured {name} hook.` to `Configured {name} hook` in `src/cli/setup_hooks.rs`, rebuild, rerun `bats --tap -f 'parity: setup-hooks' tests/native_parity.bats`: `not ok 1` with the `Configured post-merge hook.` line in the diff; revert and rebuild.

Recreate the harnesses when the session scratchpad no longer holds `phase4m/`. Both take `<engine> <repo root> <out file>`; the reference (mode 0, 1, or 2, the last calling the debug binary directly) isolates git config as `tests/test_helper.bash` does and prints each hook file's mode and bytes; the pty harness (mode 0 or 2) needs `script`, which the agent sandbox refuses, and types each line after a pause.

`phase4m/tail_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: tail_reference.sh <engine 0|1|2> <repo root> <out file>
# Runs every non-interactive branch of `generate`, `shell-init`, and
# `setup-hooks` and prints status, masked output, and the hook files each call
# left behind (mode and bytes). Mode 2 calls the debug binary directly.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/work_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH
# As tests/test_helper.bash does: the developer's git config (core.hooksPath,
# for one) must not reach the scenarios.
export GIT_CONFIG_GLOBAL="$WORK/absent-gitconfig" GIT_CONFIG_SYSTEM="$WORK/absent-gitconfig"
N=0
P=""
fresh() {
    N=$((N + 1))
    P="$WORK/p$N"
    mkdir -p "$P"
    (cd "$P" && git init --quiet && git config user.email t@t && git config user.name T)
    cd "$P" || exit 1
}
bash_init() { AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" init --no-detect --no-templates --yes --no-sync "$@" >/dev/null 2>&1; }
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    text=${text//"$WORK"/<work>}
    printf '%s\n' "$text"
}
engine() {
    if [[ "$MODE" == 2 ]]; then
        AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" "$@"
    else
        bash "$REPO/bin/agentsync.sh" "$@"
    fi
}
show() {
    local f
    for f in "$@"; do
        if [[ -f "$P/$f" ]]; then
            printf -- '--- %s mode=%s\n' "$f" "$(stat -f%Lp "$P/$f")"
            cat "$P/$f"
            printf -- '--- end %s\n' "$f"
        else
            printf -- '--- %s absent\n' "$f"
        fi
    done
}
report() {
    local name="$1" rc="$2" output="$3"
    {
        printf '### %s\n' "$name" | mask
        echo "rc=$rc"
        printf '%s' "$output" | mask
        echo
    } >> "$OUT"
}
run() {
    local name="$1"; shift
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1 < /dev/null) || rc=$?
    report "$name :: agentsync $*" "$rc" "$output"
}
run_env() {
    local name="$1" assignment="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && env "$assignment" bash -c 'exec "$@"' _ "$REPO/bin/agentsync.sh" "$@" 2>&1 < /dev/null) || rc=$?
    report "$name :: $assignment agentsync $*" "$rc" "$output"
}
run_show() {
    local name="$1" files="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1 < /dev/null) || rc=$?
    local shown; shown=$(cd "$P" && show $files)
    report "$name :: agentsync $*" "$rc" "$output"$'\n'"$shown"
}
# stdout and stderr apart, for generate's decoration split.
run_split() {
    local name="$1"; shift
    local rc=0
    (cd "$P" && engine "$@" > "$WORK/out.txt" 2> "$WORK/err.txt" < /dev/null) || rc=$?
    report "$name :: agentsync $*" "$rc" "--- stdout"$'\n'"$(cat "$WORK/out.txt")"$'\n'"--- stderr"$'\n'"$(cat "$WORK/err.txt")"
}

# ── generate ─────────────────────────────────────────────────────────────────
fresh
run_split "generate-piped" generate
run_split "generate-context" generate "Flutter app with BLoC"
run_split "generate-words" generate React + TypeScript
run_split "generate-alias" gen ctx
run_split "generate-empty-context" generate ""

# ── shell-init ───────────────────────────────────────────────────────────────
run_split "shell-init-zsh" shell-init zsh
run_split "shell-init-bash" shell-init bash
run_split "shell-init-help" shell-init --help
run_split "shell-init-help-short" shell-init -h
run_split "shell-init-fish" shell-init fish
run_split "shell-init-extra" shell-init zsh extra
if [[ "$MODE" == 2 ]]; then
    for pair in "SHELL=/usr/bin/zsh:detect-zsh" "SHELL=/bin/bash:detect-bash" "SHELL=:detect-none" "SHELL=zsh:detect-bare" "SHELL=/usr/local/bin/fish:detect-fish"; do
        a="${pair%%:*}"; n="${pair##*:}"
        rc=0; output=$(cd "$P" && env "$a" AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" shell-init 2>&1 < /dev/null) || rc=$?
        report "shell-init-$n :: $a agentsync shell-init" "$rc" "$output"
    done
else
    for pair in "SHELL=/usr/bin/zsh:detect-zsh" "SHELL=/bin/bash:detect-bash" "SHELL=:detect-none" "SHELL=zsh:detect-bare" "SHELL=/usr/local/bin/fish:detect-fish"; do
        a="${pair%%:*}"; n="${pair##*:}"
        rc=0; output=$(cd "$P" && env "$a" bash "$REPO/bin/agentsync.sh" shell-init 2>&1 < /dev/null) || rc=$?
        report "shell-init-$n :: $a agentsync shell-init" "$rc" "$output"
    done
fi

# ── setup-hooks ──────────────────────────────────────────────────────────────
fresh; bash_init --outputs local
run "hooks-help" setup-hooks --help
run "hooks-unknown" setup-hooks --bogus
run "hooks-unknown-then-help" setup-hooks --bogus --help
run_show "hooks-local" ".git/hooks/post-merge .git/hooks/post-checkout .git/hooks/pre-commit" setup-hooks
run_show "hooks-local-again" ".git/hooks/post-merge" setup-hooks
run_show "hooks-local-pre-commit" ".git/hooks/pre-commit" setup-hooks --pre-commit
printf '#!/bin/sh\necho "existing hook"\n' > "$P/.git/hooks/post-rewrite"
mv "$P/.git/hooks/post-rewrite" "$P/.git/hooks/post-merge"
run_show "hooks-local-existing" ".git/hooks/post-merge" setup-hooks
fresh; bash_init --outputs committed
run_show "hooks-committed" ".git/hooks/pre-commit .git/hooks/post-merge" setup-hooks
run_show "hooks-committed-again" ".git/hooks/pre-commit" setup-hooks --pre-commit
fresh; bash_init --outputs local
printf 'gitignore:\n  update: false\n' > "$P/.ai/agent_sync.yaml"
run_show "hooks-committed-by-gitignore" ".git/hooks/pre-commit" setup-hooks
printf 'outputs: "committed"\n' > "$P/.ai/agent_sync.yaml"
mv "$P/.ai/agent_sync.yaml" "$P/agent_sync.yaml"
run_show "hooks-legacy-config-quoted" ".git/hooks/pre-commit" setup-hooks
fresh; bash_init --outputs local
mkdir -p "$P/.githooks"
git -C "$P" config core.hooksPath .githooks
run_show "hooks-hookspath" ".githooks/post-merge .git/hooks/post-merge" setup-hooks
git -C "$P" config core.hooksPath "$P/.git/hooks"
run_show "hooks-hookspath-same" ".git/hooks/post-merge" setup-hooks
fresh; bash_init --outputs local
rm -rf "$P/.git"
run "hooks-no-git" setup-hooks
mkdir -p "$P/sub"
rc=0; output=$(cd "$P/sub" && AGENTSYNC_REPO_ROOT="$P/nowhere" engine setup-hooks 2>&1 < /dev/null) || rc=$?
report "hooks-root-missing :: (in sub, AGENTSYNC_REPO_ROOT=<root>/nowhere) agentsync setup-hooks" "$rc" "$output"
fresh; bash_init --outputs local
mkdir -p "$P/sub"
rc=0; output=$(cd "$P/sub" && engine setup-hooks 2>&1 < /dev/null) || rc=$?
report "hooks-from-subdir :: (in sub) agentsync setup-hooks" "$rc" "$output"$'\n'"$(show .git/hooks/post-merge)"
```

`phase4m/generate_tty.sh`:

```bash
#!/usr/bin/env bash
# Usage: generate_tty.sh <engine 0|2> <repo root> <out file>
# Drives `generate`'s menu on a pseudo-terminal through `script`, one line per
# 0.4 s, and records the merged transcript of each scenario. Mode 2 calls the
# debug binary directly.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/tty_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK/p"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
unset AGENTSYNC_REPO_ROOT
export PATH="$WORK/bin:$PATH"
mkdir -p "$WORK/bin"
printf '#!/bin/sh\ncat > /dev/null\n' > "$WORK/bin/pbcopy"
chmod +x "$WORK/bin/pbcopy"
if [[ "$MODE" == 2 ]]; then
    CMD="AGENTSYNC_ENGINE_VERSION=$(cat "$REPO/VERSION") $REPO/target/debug/agentsync generate"
else
    CMD="bash $REPO/bin/agentsync.sh generate"
fi
# Feed the lines one at a time; `|` in the keys string separates them.
feed() {
    local keys="$1" k
    sleep 0.6
    local -a parts=()
    if [[ -n "$keys" ]]; then
        IFS='|' read -ra parts <<< "$keys"
    fi
    for k in ${parts[@]+"${parts[@]}"}; do
        printf '%s\n' "$k"
        sleep 0.4
    done
    sleep 0.6
}
scenario() {
    local name="$1" keys="$2"
    local transcript="$WORK/$name.txt"
    (cd "$WORK/p" && feed "$keys" | script -q "$transcript" bash -c "stty -echo; $CMD; echo \"rc=\$?\"" > /dev/null 2>&1)
    {
        echo "### $name :: keys=$keys"
        tr -d '\r' < "$transcript" | sed 's/\x1b\[[0-9;]*[A-Za-z]//g' | grep -v '^$' | head -40
        echo
    } >> "$OUT"
}
scenario "choice-default" ""
scenario "choice-one" "1"
scenario "choice-invalid-then-two" "x|2|Rust CLI|with clap|"
scenario "choice-two-blank-lines" "2|first||second||"
```

```bash
bash phase4m/tail_reference.sh 0 "$PWD" phase4m/ref_bash.out && bash phase4m/tail_reference.sh 1 "$PWD" phase4m/ref_native.out
wc -l < phase4m/ref_native.out
diff phase4m/ref_bash.out phase4m/ref_native.out | grep -c '^[<>]'
bash phase4m/generate_tty.sh 0 "$PWD" phase4m/tty_bash.out && bash phase4m/generate_tty.sh 2 "$PWD" phase4m/tty_native.out
diff phase4m/tty_bash.out phase4m/tty_native.out | grep -c '^[<>]'
```

Expected: `1884` lines; `0` differing lines; `0` differing lines over the four menu scenarios (the default choice, `1`, an invalid choice then `2` with two lines, and blank lines inside the description).

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/generate.rs src/cli/shell_init.rs src/cli/setup_hooks.rs src/log.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-4m-tail.md
git commit -m "feat(native): port generate, shell-init, and setup-hooks"
```

---

### Task 2: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Spec**

Append to "Known quirks":

```markdown
52. `generate` ends with status 1 and no message when stdin closes before the
    menu choice or the description is complete.
53. `setup-hooks` reads its options in order and refuses the first unknown
    one, so `--bogus --help` prints the unknown-option error, not the help.
```

Append to "Accepted deviations":

```markdown
- Phase 4m: `generate`'s clipboard tip names the first of `pbcopy`,
  `wl-copy`, `xclip`, `xsel` the binary finds on `PATH`, as `command -v`
  found it; the tip is printed only on a terminal.
- Phase 4m: `shell-init`'s refusals are log lines coloured from stdout, as
  `_use_colors` decided, through `Log::capturing`.
```

Append to the spec's `Status:` paragraph: `Phase 4 is closed in thirteen command-family plans, the last being docs/plans/2026-09-16-rust-migration-phase-4m-tail.md.`

- [x] **Step 2: Module map and outputs**

Set the `lib/helpers/generate.sh` row to `→ src/cli/generate.rs     Phase 4m, ported; prompt embedded`, the `lib/helpers/shell_init.sh` row to `→ src/cli/shell_init.rs   Phase 4m, ported; stdout carries the snippet alone`, and the `lib/setup_hooks.sh` row to `→ src/cli/setup_hooks.rs  Phase 4m, ported; git through the executable`. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase4i/native_suite.sh "$PWD" both phase4m/suite_both.out && tail -1 phase4m/suite_both.out
```

Expected: `285 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 50 bats files, each run one at a time under both engines: the Phase 4 exit.

- [x] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-4m-tail.md
git commit -m "docs(native): map the phase 4m modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the three parity fixtures pass, and a `## Completion receipt` records the fresh verification. With it Phase 4 is closed: `_NATIVE_COMMANDS` names every command but `help`, `update`, and `release`, which Phase 5 owns, and the whole suite passes under `AGENTSYNC_NATIVE=1`.

## Completion receipt

### Decisions the review took

All three as recommended, on 2026-09-16, under the maintainer's standing instruction to run Phase 4 to its close; the quirks were reproduced with `tail_reference.sh` and `generate_tty.sh` before being recorded.

### Global Constraints

| Constraint | Satisfied by |
|---|---|
| `generate` and `shell-init` write nothing; `setup-hooks` writes only below the hooks directory git names | `src/cli/generate.rs` and `src/cli/shell_init.rs` call no filesystem writer; `src/cli/setup_hooks.rs` writes through `install_hook` under `hooks_dir` alone, after `physical()` confirmed it is git's own |
| No Bash change; `lib/**/*.sh` and `bin/agentsync.sh` clean under ShellCheck | `git diff --stat <plan>..HEAD -- lib bin` names only `bin/agentsync.sh` (the `_NATIVE_COMMANDS` line); ShellCheck exit 0 |
| Byte-for-byte parity except the accepted deviations | `tests/native_parity.bats`: the `generate`, `shell-init`, and `setup-hooks` fixtures with `_assert_same_hooks`; the reference and pty transcripts in Task 1 Step 5 |
| `unsafe_code = "forbid"`, fmt and clippy clean, no new dependency; `main.rs` alone reads the environment and terminal state; only `git` is spawned | `Cargo.toml` unchanged; `SHELL`, `PATH`, `AGENTSYNC_REPO_ROOT`, `PWD`, and both terminal states are read in `src/main.rs`; `Command::new("git")` in `setup_hooks.rs` is the one new spawn |
| Disk-touching unit tests are `#[cfg(unix)]`, and the `setup-hooks` tests pin `core.hooksPath` | the `setup_hooks.rs` tests module is `#[cfg(all(test, unix))]` and its `repo()` sets `core.hooksPath .git/hooks` |
| Expected values captured from Bash | `tail_reference.sh` (1884 lines) and `generate_tty.sh` (4 scenarios), reproduced in Task 1 Step 5 |
| Conventional Commits, at most 72 characters, no trailers | `d5711ea`, `5ecb846`, the map commit, and the close commit |
| bats one file at a time | every recorded run, including `native_suite.sh` |

### Fresh verification, 2026-09-16, macOS 26.5 arm64, outside the agent sandbox where noted

- `cargo test`: 285 passed (lib), 0 passed (doc), 11 passed (cli), 1 passed (interrupt).
- `cargo clippy --all-targets -- -D warnings`: exit 0. `cargo fmt --all --check`: exit 0.
- `shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh`: exit 0.
- bats, one file at a time under both engines with `native_suite.sh` in a detached worktree at `5ecb846` (the port commit; the map commit that follows changes no code or test) with the release binary built there: 50 files, `TOTAL bash=0 native=0`, `native_parity` (65 cases) included and run outside the sandbox: the Phase 4 exit.
- Mutation in Task 1 Step 5: `Configured {name} hook.` → `Configured {name} hook` failed the setup-hooks fixture on the `Configured post-merge hook.` line; reverted, rebuilt, byte-identical to the verified draft.
- Against the `5ecb846` tree: `tail_reference.sh` gave 1884-line transcripts for both engines with 0 differing lines, git config isolated; `generate_tty.sh` on a pty gave identical transcripts for the four menu scenarios.
- `sync` and `check` are unchanged; no timings.

### Skipped, deferred, open

- **Windows**: native bats runs stay off Windows until Phase 5; `cargo test` still runs there.
- **`help`, `update`, and `release`** stay in Bash by decision 2 and the spec's Phase 5.
- **Not pushed.** The cutover (Phase 5) is the first release point.

## Run log

### 2026-09-16 — Phase 4m planned
- Commits: this plan.
- Verified: every non-interactive branch of the three commands was captured with `tail_reference.sh` (1884 lines, git config isolated so the developer's `core.hooksPath` did not reach the scenarios) and the menu with `generate_tty.sh` (4 scenarios on a pty). No Bash bug turned up. The Rust in Task 1 was drafted in the tree: `cargo test` 285/0/11/1 with the 4k and 4l ports, fmt and clippy clean; the debug binary, called directly, gave transcripts identical to Bash on both harnesses. Baseline `cargo test` 277/0/11/1; `generate.bats` 7, `shell_init.bats` 15, `hooks.bats` 16, `native_parity.bats` 62 cases green in Bash.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-16 — phase closed
- Commits: `d892e34` docs(native): map the phase 4m modules and quirks; this commit, docs(native): close phase 4m.
- Verified: `cargo test` 285/0/11/1; clippy, fmt, ShellCheck exit 0; `native_suite.sh both` over 50 bats files `TOTAL bash=0 native=0` in the worktree at `5ecb846`; the reference and pty transcripts as recorded in Task 1 Step 5.
- Plan amended: none.
- Next: Phase 5, distribution and cutover: its plan, per the spec's Phase 5 section.
- Blocker: none.
