# Rust Migration Phase 4i: Native `init`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync init` so the binary answers it byte for byte like `cmd_init` in `lib/helpers/init.sh`: argument parsing, marker detection, the interactive wizard with `prompt_multiselect`, the backup transaction with its restore, template and payload scaffolding, the project config, the manifest heal, adoption of the tool config a project already has, the CI gate, and the first sync. Four Bash bugs the reference turned up are fixed first.

**Architecture:** `prompts.rs` gains `Multiselect` (the list, its frames, and its key handling), `multiselect` (the loop over a key source), and `RawTerminal` (the terminal device in the mode `read -rsn1` needs, set through `stty` and restored on drop). `catalog` gains `base_payloads` (every `<resource>/<slug>.*`) and the embedded CI workflow. `src/cli/init.rs` holds the command; `main` hands it an `Env` carrying the version, the logical working directory, the backup bounds, `prompts::is_tty`, `prompts::confirm`, `prompts::multiselect_on_terminal`, and a closure that runs `cli::sync::run` for the first sync. The adopt resolver from 4f serves the adoption; `write_template` from 4h writes the templates. The seam stays the CLI process boundary: `tests/init.bats` and `tests/init_flow.bats` under `AGENTSYNC_NATIVE=1`, two tree-parity fixtures, and, since every seeded bats file runs `init`, the whole suite under `AGENTSYNC_NATIVE=1`.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4h-refresh.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `init` writes only below `.ai/` (source templates, payload overrides, the config, the template manifest, the backup store), the adopted copies of existing outputs into `.ai/src/`, the CI workflow when asked, and what the first sync writes; `--dry-run` writes nothing; a failure after the backup restores the pre-init state.
- No binary ships to users; without a binary every command runs in Bash. Four Bash changes, Tasks 1–4, each in its own commit with a regression test; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state; the terminal mode is switched through `stty`, never through raw ioctls.
- Disk-touching unit tests are `#[cfg(unix)]`.
- No bats fixture opens the wizard: every fixture runs off a terminal, where `init` takes its defaults; the wizard is proven on a pseudo-terminal in Task 6 Step 5, with keys typed one at a time, because a byte written before `read -rsn1` switches the terminal to raw mode sits in the canonical line buffer.
- Every expected value was captured from the fixed Bash on 2026-09-15 and 2026-09-16: `phase4i/init_reference.sh` (53 non-interactive scenarios, a 2862-line transcript with trees, hashes, modes, and each backup's targets) and `phase4i/init_tty.sh` (9 wizard scenarios on a pty through `script`, 753 lines). Both scripts, and `phase4i/native_suite.sh`, are reproduced in Task 6 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-16 under the maintainer's standing instruction to run Phase 4 to its close; each was checked against Bash before the call.

1. **Fix four Bash bugs before porting.** Each was reproduced on the committed engine and each regression test fails there.
   - **`init <missing-dir>` dies on an unbound `$1`.** After the option loop `$1` is empty, so `set -u` aborts with `init.sh: line 921: $1: unbound variable` and the run exits 0 having done nothing. **Fix:** print `Directory not found: <requested>` and exit 1 (Task 1).
   - **Two destinations that share a source keep the wrong one.** `adopt_file_quiet` copies before `_init_adopt_existing` checks the claim, so with `AGENTS.md` and `CLAUDE.md` both present the output reports `Adopted AGENTS.md` and `Kept as-is CLAUDE.md`, while `.ai/src/AGENTS.md` holds `CLAUDE.md`'s content. The same `$(...)` subshell loses `ADOPT_QUIET_REASON`, so every refusal read `not an adoptable output`. **Fix:** resolve first, then copy only an unclaimed source, and print the resolver's reason; `adopt_file_quiet` goes (Task 2).
   - **The wizard never shows its lists.** `prompt_multiselect` tests `is_tty`, stdin and stdout, but `init` captures its stdout, so on a terminal both lists return their defaults at once and the typed keys land in the next question. **Fix:** the list is drawn on stderr and read from `/dev/tty`, so the test is stdin and stderr (Task 3).
   - **Arrow keys cancel the wizard under Bash 3.2.** `read -t 0.01` is `invalid timeout specification` on macOS's Bash, so every escape sequence reads as a lone Escape. **Fix:** `-t 1`, the smallest timeout Bash 3.2 accepts; a lone Escape registers after a second (Task 4).
   Alternative: port the behaviour as is. The first two lose user work, the last two make the documented wizard unusable, so they are not offered as quirks.
2. **Raw terminal mode through `stty`.** `read -rsn1` sets the terminal in-process; without `libc` and with `unsafe_code = "forbid"` the binary runs `stty -icanon -echo min 1 time 0` on the terminal device, saves the settings with `stty -g`, and restores them on drop; an Escape is followed by two reads under `min 0 time 10`, the same second Bash waits. Recorded as an accepted deviation. Alternative: a `nix` or `libc` dependency for `tcsetattr`, rejected by the no-new-dependency constraint.
3. **Quirks and deviations.** Record as known quirks 41–43: a detected tool whose destination has a file where a directory is expected, such as a legacy single-file `.clinerules`, makes `init` refuse at the backup with `Backup target parent is not a directory`; `init` drops every space inside a `--tools` or `--content` token and `--tools=` skips the wizard while contributing nothing; the template manifest is healed before adoption, so an adopted `AGENTS.md` carries the template's hash and `refresh` treats it as a silently kept edit. Record as accepted deviations the `stty` mode switch, the Rust I/O message where Bash printed the shell's redirect error on a failed scaffold write, and the first sync running in-process where Bash spawned `sync.sh`. **Recommended:** as listed.

## Module closure

```text
lib/helpers/init.sh              19-63    content constants, csv and list helpers
                                 66-81    _init_configure_resolver (Project::at and Paths::on_disk)
                                 86-147   _init_existing_dest_files, _init_adopt_existing (Task 2)
                                 151-171  _init_write_ci_workflow
                                 173-236  _init_collect_backup_targets, _init_cleanup, the traps
                                 238-370  _init_create_directories, _init_copy_source_templates, _init_copy_tool_payloads
                                 372-410  _init_detect_enabled_tools
                                 412-485  _init_create_project_config
                                 487-607  _init_print_summary
                                 609-636  _init_validate_content, _init_list_available_tools
                                 638-690  _init_print_plan
                                 742-1172 cmd_init (Task 1 at 919-923)
lib/helpers/prompts.sh           10-12    is_tty; 41-168 prompt_multiselect (Tasks 3-4)
lib/helpers/adopt.sh             39-60    _adopt_prepare_context; 352-395 _adopt_resolve_dest; 553-571 adopt_file_quiet (removed in Task 2)
lib/helpers/template_manifest.sh 146-191  template_manifest_heal_from_match (ported in 4h)
lib/helpers/backup.sh            27-52    backup_configure; 353 backup_create; 551 backup_restore; 685 backup_prune (ported in 3 and 3b)
lib/helpers/backup_state.sh      221      backup_seal (ported in 3b)
lib/templates/ci/github-agentsync-check.yml   embedded as catalog::CI_GITHUB_WORKFLOW
bin/agentsync.sh                 280      _NATIVE_COMMANDS; 354 init's _need list
```

Reused: `Project::at`, `Tool::{load, flag, value}`, `render::TARGET_KEYS`, `Paths::{on_disk, resolve_dest}`, `paths::{normalize, ai_dir_enclosing_root, leaf}`, `project_config::{select, missing_message}`, `backup::{configure, create, restore, prune, Retention}`, `witness::seal`, `interrupt::Interrupt`, `TemplateManifest::{load, heal_from_match, write}`, `catalog::{base_tools, template_files}`, `adopt::{discover_sources, Resolver, copy_into_source}`, `refresh::write_template`, `format_rev::engine`, `staging::write_beside`, `cli::sync::run`, `prompts::{is_tty, confirm}`, `style`. The `rules/*.md`, `commands/*.md`, `agents/*.md`, and `skills/*/` globs and `_init_list_available_tools`'s `sort` follow the locale; the shipped names are lowercase ASCII, so byte order gives the same listing (Phase 2 deviation).

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in init init_flow adopt native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the plan's latest commit; `243 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses.

---

### Task 1: A missing target directory is named

**Files:**
- Modify: `lib/helpers/init.sh` (`cmd_init`, the `cd` into the target)
- Test: `tests/init.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Add to `tests/init.bats`, before `init does not copy system engine into project`:

```bash
@test "init names a missing target directory and fails" {
    run run_agentsync init missing-dir
    [ "$status" -eq 1 ]
    [[ "$output" == *"Directory not found: missing-dir"* ]]
    [ ! -d ".ai" ]
    [ ! -d "missing-dir" ]
}

```

Run: `bats --tap -f 'missing target' tests/init.bats`
Expected: `not ok 1 init names a missing target directory and fails`.

- [x] **Step 2: Name the requested directory**

In `cmd_init`, replace the `target_dir="${target_dir:-.}"` line and the `cd` block that follows it with:

```bash
    local requested_dir="${target_dir:-.}"
    target_dir="$(cd "$requested_dir" 2>/dev/null && pwd)" || {
        echo "$(_red "Error"): Directory not found: $requested_dir" >&2
        exit 1
    }
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/init.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/init.sh
```

Expected: `36`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/init.sh tests/init.bats docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "fix(init): name the missing target directory"
```

---

### Task 2: The first of two files that share a source is the one adopted

**Files:**
- Modify: `lib/helpers/init.sh` (`_init_adopt_existing`), `lib/helpers/adopt.sh` (`adopt_file_quiet` removed)
- Test: `tests/init_flow.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing tests**

In `tests/init_flow.bats`, replace `init: two destinations mapping to one source keep the first and report the rest` with these two tests:

```bash
@test "init: two destinations mapping to one source keep the first and report the rest" {
    printf '# From CLAUDE\n' > CLAUDE.md
    printf '# From AGENTS\n' > AGENTS.md
    run run_agentsync init --tools claude,codex --yes --no-sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"Adopted AGENTS.md"* ]]
    [[ "$output" == *"Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md"* ]]
    [ "$(cat .ai/src/AGENTS.md)" = "# From AGENTS" ]
}

@test "init: a file no tool produces is kept with the resolver's reason" {
    mkdir -p .cursor/rules
    printf 'body\n' > .cursor/rules/core.mdc
    run run_agentsync init --tools cursor --yes --no-sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync."* ]]
    [ "$(cat .cursor/rules/core.mdc)" = "body" ]
}
```

Run: `bats --tap -f 'two destinations|resolver' tests/init_flow.bats`
Expected: `not ok 1 init: two destinations mapping to one source keep the first and report the rest` and `not ok 2 init: a file no tool produces is kept with the resolver's reason`.

- [x] **Step 2: Resolve, check the claim, then copy**

Replace `_init_adopt_existing` in `lib/helpers/init.sh` with:

```bash
_init_adopt_existing() {
    local target_dir="$1"
    local existing="$2"   # newline-separated repo-relative paths

    local claimed="|" file adopted=0 skipped=0
    local -a skips=()
    while IFS= read -r file; do
        [[ -n "$file" ]] || continue
        _adopt_resolve_dest "$target_dir/$file"
        if [[ -n "$_ADOPT_REFUSAL" ]]; then
            skips+=("$file — $_ADOPT_REFUSAL")
            skipped=$((skipped + 1))
            continue
        fi
        if [[ "$claimed" == *"|$_ADOPT_SOURCE_REL|"* ]]; then
            skips+=("$file — another file already became $_ADOPT_SOURCE_REL")
            skipped=$((skipped + 1))
            continue
        fi
        ensure_dir "$(dirname "$_ADOPT_SOURCE_ABS")"
        cp "$_ADOPT_DEST_ABS" "$_ADOPT_SOURCE_ABS"
        claimed+="$_ADOPT_SOURCE_REL|"
        echo "   $(_green "Adopted") $(_cyan "$file") → $(_dim "$_ADOPT_SOURCE_REL")"
        adopted=$((adopted + 1))
    done <<< "$existing"

    local note
    for note in "${skips[@]+"${skips[@]}"}"; do
        echo "   $(_yellow "Kept as-is") $note"
    done
    if [[ $skipped -gt 0 ]]; then
        echo "   $(_dim "Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.")"
    fi
    [[ $adopted -gt 0 || $skipped -gt 0 ]] && echo ""
    return 0
}
```

In `lib/helpers/adopt.sh`, delete `adopt_file_quiet`, the `ADOPT_QUIET_REASON` global, and their comment block, leaving `cmd_adopt` to follow `_adopt_all` directly:

```diff
diff --git a/lib/helpers/adopt.sh b/lib/helpers/adopt.sh
index af74c8f..a1009a7 100644
--- a/lib/helpers/adopt.sh
+++ b/lib/helpers/adopt.sh
@@ -546,30 +546,6 @@ _adopt_all() {
     _adopt_all_apply
 }
 
-# Copy <dest> into its .ai/src/ source with no prompt, diff, or manifest write —
-# the pre-first-sync path `agentsync init` uses to keep a project's existing
-# config. Echoes the source's repo-relative path; on refusal returns 1 with the
-# reason in ADOPT_QUIET_REASON. Callers own the resolver globals.
-# Read by init.sh after a failed adopt_file_quiet.
-# shellcheck disable=SC2034
-ADOPT_QUIET_REASON=""
-adopt_file_quiet() {
-    local dest="$1"
-    # shellcheck disable=SC2034
-    ADOPT_QUIET_REASON=""
-
-    _adopt_resolve_dest "$dest"
-    if [[ -n "$_ADOPT_REFUSAL" ]]; then
-        # shellcheck disable=SC2034  # read by init.sh
-        ADOPT_QUIET_REASON="$_ADOPT_REFUSAL"
-        return 1
-    fi
-
-    ensure_dir "$(dirname "$_ADOPT_SOURCE_ABS")"
-    cp "$_ADOPT_DEST_ABS" "$_ADOPT_SOURCE_ABS"
-    echo "$_ADOPT_SOURCE_REL"
-}
-
 cmd_adopt() {
     local dry_run="false"
     local assume_yes="false"
```

- [x] **Step 3: Run the tests, confirm green**

```bash
for f in init init_flow adopt; do
    printf '%s ok=%s notok=%s\n' "$f" "$(bats --tap "tests/$f.bats" | grep -c '^ok')" "$(bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
shellcheck -x -S warning -e SC1091 lib/helpers/init.sh lib/helpers/adopt.sh
```

Expected: `init ok=36 notok=0`, `init_flow ok=15 notok=0`, `adopt ok=29 notok=0`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/init.sh lib/helpers/adopt.sh tests/init_flow.bats docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "fix(init): adopt the first of two files that share a source"
```

---

### Task 3: The wizard draws its lists when stdout is captured

**Files:**
- Modify: `lib/helpers/prompts.sh` (`prompt_multiselect`'s terminal test)
- Test: `tests/init.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Add to `tests/init.bats`, before `init names a missing target directory and fails` (Task 4 adds the arrow):

```bash
@test "init: the wizard draws its tool list on a terminal" {
    command -v script >/dev/null 2>&1 || skip "script(1) not available"
    # Enter through both lists, keep committed outputs, decline at Proceed.
    local keys=$'\n\ny\nn\n'
    if script --version >/dev/null 2>&1; then
        run bash -c 'printf "%s" "$1" | script -q -c "AGENTSYNC_HOME=\"$2\" bash \"$3\" init" /dev/null' _ "$keys" "$REPO_ROOT" "$AGENTSYNC_BIN"
    else
        run bash -c 'printf "%s" "$1" | script -q /dev/null bash "$3" init' _ "$keys" "$REPO_ROOT" "$AGENTSYNC_BIN"
    fi
    [[ "$output" == *"Tools to enable"* ]]
    [[ "$output" == *"(space: toggle"* ]]
    [[ "$output" == *"Content sections:"* ]]
    [[ "$output" == *"Cancelled."* ]]
    [ ! -d ".ai" ]
}
```

Run outside the agent sandbox, which refuses `script`'s pseudo-terminal: `bats --tap -f 'wizard' tests/init.bats`
Expected: `not ok 1 init: the wizard draws its tool list on a terminal`.

- [x] **Step 2: Test stdin and stderr**

Replace the `# Non-TTY fast path.` block at the top of `prompt_multiselect` with:

```bash
    # Callers capture stdout for the selection, so the terminal test is stdin
    # and stderr: the list is drawn on stderr and the keys come from /dev/tty.
    if [[ ! -t 0 ]] || [[ ! -t 2 ]]; then
        echo "$preselected"
        return 0
    fi
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/init.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/prompts.sh
```

Expected: `37`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/prompts.sh tests/init.bats docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "fix(prompts): draw the multiselect when stdout is captured"
```

---

### Task 4: An escape sequence is read under Bash 3.2

**Files:**
- Modify: `lib/helpers/prompts.sh` (`prompt_multiselect`'s escape reads)
- Test: `tests/init.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

In the wizard test, replace the key line and its comment with:

```bash
    # Move down once in the tool list (an arrow must not read as Escape), Enter
    # through both lists, keep committed outputs, decline at Proceed.
    local keys=$'\e[B\n\ny\nn\n'
```

Run outside the agent sandbox: `bats --tap -f 'wizard' tests/init.bats`
Expected: `not ok 1 init: the wizard draws its tool list on a terminal` (`Content sections:` is missing: the arrow cancelled the first list).

- [x] **Step 2: Wait a whole second**

Replace the two `-t 0.01` reads and their comment with:

```bash
        # Handle escape sequences for arrow keys. Bash 3.2 rejects a
        # fractional -t, so a lone Escape takes a second to register.
        if [[ "$key" == $'\033' ]]; then
            local k2 k3
            IFS= read -rsn1 -t 1 k2 </dev/tty || k2=""
            IFS= read -rsn1 -t 1 k3 </dev/tty || k3=""
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/init.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/prompts.sh
```

Expected: `37`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/prompts.sh tests/init.bats docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "fix(prompts): wait a whole second for an escape sequence"
```

---

### Task 5: Port the multiselect prompt

**Files:**
- Modify: `src/prompts.rs`

**Interfaces:**
- Produces:
  - `pub enum prompts::Key { Up, Down, Toggle, All, Clear, Enter, Cancel, Other }`
  - `pub struct prompts::Cancelled(pub Vec<String>)`
  - `pub struct prompts::Multiselect` with `new(title: &str, options: &[String], preselected: &[String])`, `frame(&mut self, style: &Style) -> String`, `press(&mut self, key: Key) -> Option<Result<Vec<String>, Cancelled>>`, `picked(&self) -> Vec<String>`
  - `pub fn prompts::multiselect(title, options, preselected, interactive: bool, style, keys: &mut dyn FnMut() -> Key, err: &mut dyn Write) -> Result<Vec<String>, Cancelled>`
  - `pub fn prompts::multiselect_on_terminal(title, options, preselected, style) -> Result<Vec<String>, Cancelled>`
  - `pub struct prompts::RawTerminal` with `open() -> Option<Self>`, `key(&mut self) -> Key`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module of `src/prompts.rs`:

```rust
    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_list_draws_and_moves_like_prompt_multiselect() {
        let style = Style::plain();
        let mut list = Multiselect::new("Pick:", &strings(&["a", "b", "c"]), &strings(&["b"]));
        assert_eq!(
            list.frame(&style),
            "\r\x1b[KPick:\n\r\x1b[K  (space: toggle · a: all · n: none · enter: confirm)\n\r\x1b[K › [ ] a\n\r\x1b[K   [x] b\n\r\x1b[K   [ ] c\n"
        );
        assert_eq!(list.press(Key::Up), None);
        assert!(list.frame(&style).starts_with("\x1b[5A\r\x1b[KPick:\n"));
        assert!(list.frame(&style).ends_with("\r\x1b[K › [ ] c\n"));
        assert_eq!(list.press(Key::Toggle), None);
        assert_eq!(list.press(Key::Down), None);
        assert_eq!(list.press(Key::Other), None);
        assert_eq!(list.picked(), strings(&["b", "c"]));
        assert_eq!(list.press(Key::Toggle), None);
        assert_eq!(list.press(Key::Enter), Some(Ok(strings(&["a", "b", "c"]))));
        assert_eq!(list.press(Key::Clear), None);
        assert_eq!(list.press(Key::Enter), Some(Ok(Vec::new())));
        assert_eq!(list.press(Key::All), None);
        assert_eq!(
            list.press(Key::Cancel),
            Some(Err(Cancelled(strings(&["b"]))))
        );
    }

    #[test]
    fn the_prompt_hides_the_cursor_reads_keys_and_returns_preselected_off_a_terminal() {
        let style = Style::plain();
        let options = strings(&["x", "y"]);
        let mut err = Vec::new();
        let mut never = || panic!("no key is read off a terminal");
        assert_eq!(
            multiselect(
                "T",
                &options,
                &strings(&["y"]),
                false,
                &style,
                &mut never,
                &mut err
            ),
            Ok(strings(&["y"]))
        );
        assert!(err.is_empty());

        let mut keys = [Key::Toggle, Key::Enter].into_iter();
        let mut next = || keys.next().unwrap();
        assert_eq!(
            multiselect("T", &options, &[], true, &style, &mut next, &mut err),
            Ok(strings(&["x"]))
        );
        let shown = String::from_utf8(err).unwrap();
        assert!(shown.starts_with("\x1b[?25l\r\x1b[KT\n"));
        assert!(shown.contains("\x1b[4A\r\x1b[KT\n"));
        assert!(shown.ends_with(" › [x] x\n\r\x1b[K   [ ] y\n\x1b[?25h"));
        assert_eq!(
            multiselect("T", &[], &[], true, &style, &mut never, &mut Vec::new()),
            Ok(Vec::new())
        );
    }
```

Run: `cargo test --lib 2>&1 | grep -E '^error\[E0(412|422|425|433)\]' | sort -u | head -6`
Expected: errors naming the missing `Multiselect`, `Key`, `Cancelled`, and `multiselect`.

- [ ] **Step 2: Write the implementation**

Replace the head of `src/prompts.rs`, down to and including the `use` lines, with:

```rust
//! `lib/helpers/prompts.sh`: questions go to stderr and answers come from the
//! terminal device, so captured output never swallows a prompt.

use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::process::{Command, Stdio};

use crate::style::Style;

#[cfg(windows)]
const TERMINAL: &str = "CONIN$";
#[cfg(not(windows))]
const TERMINAL: &str = "/dev/tty";
```

Move the two `TERMINAL` constants out of `read_terminal_line`, which becomes:

```rust
fn read_terminal_line() -> Option<String> {
    let tty = std::fs::File::open(TERMINAL).ok()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line)
}
```

Insert after it, before the `tests` module:

```rust
/// A key `prompt_multiselect` reacts to, as `read -rsn1` delivers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Toggle,
    All,
    Clear,
    Enter,
    Cancel,
    Other,
}

/// The list was cancelled with `q` or Escape; the preselected items stand.
#[derive(Debug, PartialEq, Eq)]
pub struct Cancelled(pub Vec<String>);

/// `prompt_multiselect`'s list: what is selected, where the cursor is, and
/// whether a frame is already on screen to be overwritten.
pub struct Multiselect {
    title: String,
    options: Vec<String>,
    selected: Vec<bool>,
    preselected: Vec<String>,
    cursor: usize,
    drawn: bool,
}

impl Multiselect {
    pub fn new(title: &str, options: &[String], preselected: &[String]) -> Self {
        let selected = options
            .iter()
            .map(|option| preselected.iter().any(|p| p == option))
            .collect();
        Self {
            title: title.to_string(),
            options: options.to_vec(),
            selected,
            preselected: preselected.to_vec(),
            cursor: 0,
            drawn: false,
        }
    }

    /// `_redraw`: the next frame, drawn over the previous one after the first.
    pub fn frame(&mut self, style: &Style) -> String {
        let mut out = String::new();
        if self.drawn {
            out.push_str(&format!("\x1b[{}A", self.options.len() + 2));
        }
        self.drawn = true;
        out.push_str(&format!("\r\x1b[K{}\n", self.title));
        out.push_str(&format!(
            "\r\x1b[K{}\n",
            style.dim("  (space: toggle · a: all · n: none · enter: confirm)")
        ));
        for (i, option) in self.options.iter().enumerate() {
            let check = if self.selected[i] {
                format!("[{}]", style.green("x"))
            } else {
                "[ ]".to_string()
            };
            let (marker, name) = if i == self.cursor {
                (style.cyan("›"), style.bold(option))
            } else {
                (" ".to_string(), option.clone())
            };
            out.push_str(&format!("\r\x1b[K {marker} {check} {name}\n"));
        }
        out
    }

    /// One key; `Some` when the list is confirmed or cancelled.
    pub fn press(&mut self, key: Key) -> Option<Result<Vec<String>, Cancelled>> {
        let n = self.options.len();
        match key {
            Key::Up => self.cursor = (self.cursor + n - 1) % n,
            Key::Down => self.cursor = (self.cursor + 1) % n,
            Key::Toggle => self.selected[self.cursor] = !self.selected[self.cursor],
            Key::All => self.selected.iter_mut().for_each(|s| *s = true),
            Key::Clear => self.selected.iter_mut().for_each(|s| *s = false),
            Key::Enter => return Some(Ok(self.picked())),
            Key::Cancel => return Some(Err(Cancelled(self.preselected.clone()))),
            Key::Other => {}
        }
        None
    }

    /// The selected options, in list order.
    pub fn picked(&self) -> Vec<String> {
        self.options
            .iter()
            .zip(&self.selected)
            .filter(|(_, on)| **on)
            .map(|(option, _)| option.clone())
            .collect()
    }
}

/// `prompt_multiselect`: off a terminal the preselected items come back
/// unchanged; on one the list is drawn on `err` and driven by `keys`.
pub fn multiselect(
    title: &str,
    options: &[String],
    preselected: &[String],
    interactive: bool,
    style: &Style,
    keys: &mut dyn FnMut() -> Key,
    err: &mut dyn Write,
) -> Result<Vec<String>, Cancelled> {
    if !interactive {
        return Ok(preselected.to_vec());
    }
    if options.is_empty() {
        return Ok(Vec::new());
    }
    let mut list = Multiselect::new(title, options, preselected);
    let _ = err.write_all(b"\x1b[?25l");
    let _ = err.write_all(list.frame(style).as_bytes());
    let _ = err.flush();
    loop {
        if let Some(outcome) = list.press(keys()) {
            let _ = err.write_all(b"\x1b[?25h");
            let _ = err.flush();
            return outcome;
        }
        let _ = err.write_all(list.frame(style).as_bytes());
        let _ = err.flush();
    }
}

/// `prompt_multiselect` on the terminal device, keys read one byte at a time
/// as `read -rsn1` does. The list is drawn on stderr, so the terminal test is
/// stdin and stderr, as in Bash, whose callers capture stdout.
pub fn multiselect_on_terminal(
    title: &str,
    options: &[String],
    preselected: &[String],
    style: &Style,
) -> Result<Vec<String>, Cancelled> {
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut terminal = interactive.then(RawTerminal::open).flatten();
    let mut keys = || match terminal.as_mut() {
        Some(terminal) => terminal.key(),
        None => Key::Enter,
    };
    multiselect(
        title,
        options,
        preselected,
        interactive,
        style,
        &mut keys,
        &mut std::io::stderr(),
    )
}

/// The terminal device with echo and line buffering off, as `read -rsn1`
/// leaves it while it waits; the saved settings come back on drop.
pub struct RawTerminal {
    tty: std::fs::File,
    saved: Option<String>,
}

impl RawTerminal {
    /// `None` when the terminal device cannot be opened, as when `read
    /// </dev/tty` fails.
    pub fn open() -> Option<Self> {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(TERMINAL)
            .ok()?;
        let saved = stty(&tty, &["-g"]).map(|text| text.trim().to_string());
        stty(&tty, &["-icanon", "-echo", "min", "1", "time", "0"])?;
        Some(Self { tty, saved })
    }

    /// One keypress. An escape is followed by two reads of up to a second for
    /// `[A` and `[B`, the `read -t 1` Bash 3.2 allows; anything else after it,
    /// or nothing, cancels, as in Bash.
    pub fn key(&mut self) -> Key {
        let mut byte = [0u8; 1];
        match self.tty.read(&mut byte) {
            Ok(1) => {}
            _ => return Key::Enter,
        }
        match byte[0] {
            0x1b => {
                let _ = stty(&self.tty, &["min", "0", "time", "10"]);
                let mut seq = [0u8; 2];
                let mut got = 0;
                while got < 2 {
                    match self.tty.read(&mut seq[got..]) {
                        Ok(n) if n > 0 => got += n,
                        _ => break,
                    }
                }
                let _ = stty(&self.tty, &["min", "1", "time", "0"]);
                match &seq[..got] {
                    b"[A" => Key::Up,
                    b"[B" => Key::Down,
                    _ => Key::Cancel,
                }
            }
            b'k' => Key::Up,
            b'j' => Key::Down,
            b' ' => Key::Toggle,
            b'a' | b'A' => Key::All,
            b'n' | b'N' => Key::Clear,
            b'\n' | b'\r' => Key::Enter,
            b'q' => Key::Cancel,
            _ => Key::Other,
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            let _ = stty(&self.tty, &[saved.as_str()]);
        }
    }
}

/// `stty <args>` on the terminal device; its stdout on success.
fn stty(tty: &std::fs::File, args: &[&str]) -> Option<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(tty.try_clone().ok()?)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
```

- [ ] **Step 3: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `245 passed`, `0`, `11`, `1`; clippy exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/prompts.rs docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "feat(native): port the multiselect prompt"
```

---

### Task 6: Port `init`

**Files:**
- Create: `src/cli/init.rs`
- Modify: `src/catalog.rs`, `src/cli/adopt.rs`, `src/cli/refresh.rs`, `src/cli/mod.rs` (`pub mod init;` after `pub mod enable;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 5's `prompts::{Cancelled, multiselect_on_terminal}`; 4h's `refresh::write_template` and `TemplateManifest::heal_from_match`; 4f's `adopt::{discover_sources, Resolver, copy_into_source}`.
- Produces:
  - `pub fn catalog::base_payloads(resource: &str, slug: &str) -> Vec<&'static File<'static>>`, `pub const catalog::CI_GITHUB_WORKFLOW: &str`
  - `pub(crate) fn adopt::copy_into_source`, `pub(crate) fn refresh::write_template` (visibility only)
  - `pub type init::Picker<'a> = &'a mut dyn FnMut(&str, &[String], &[String]) -> Result<Vec<String>, Cancelled>`
  - `pub struct init::Env<'a> { version, cwd, config_path, backup_limit, backup_max_age, interactive, confirm: &mut dyn FnMut(&str, bool) -> bool, multiselect: Picker, sync: &mut dyn FnMut(&str) -> u8 }`
  - `pub fn init::init(args: &[String], style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [ ] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── init ─────────────────────────────────────────────────────────────────────
# Every call runs off a terminal, so the wizard never opens; the seed's `.ai`
# is removed first because init skips a project that already has `.ai/src`.

@test "parity: init scaffolds, detects, adopts, and runs the first sync like Bash" {
    assert_tree_parity init
    rm -rf .ai
    assert_tree_parity init --help
    assert_tree_parity init --bogus
    assert_tree_parity init a b
    assert_tree_parity init --outputs bogus
    assert_tree_parity init --existing bogus
    assert_tree_parity init --ci gitlab
    assert_tree_parity init missing-dir
    assert_tree_parity init --content bogus
    assert_tree_parity init --dry-run --tools claude,cursor --content agents,rules
    assert_tree_parity init --dry-run --no-templates --no-detect
    assert_tree_parity init --no-detect --no-sync
    assert_tree_parity init --no-templates --no-detect --content agents,rules --no-sync
    assert_tree_parity init --tools "claude, cursor,claude" --content " rules , skills " --no-detect --no-sync
    assert_tree_parity init --tools claude --yes
    assert_tree_parity init --tools claude --yes --outputs local --ci github
    mkdir -p .claude/rules .cursor/rules .github/workflows
    printf '# Hand-written team rules\n' > CLAUDE.md
    printf '# Legacy rule\n' > .claude/rules/legacy.md
    printf '{"settings": true}\n' > .claude/settings.json
    printf '# From AGENTS\n' > AGENTS.md
    printf 'body\n' > .cursor/rules/core.mdc
    printf 'name: mine\n' > .github/workflows/agentsync-check.yml
    assert_tree_parity init --dry-run
    assert_tree_parity init --yes --no-sync
    assert_tree_parity init --tools codex --yes --ci github --no-sync
    assert_tree_parity init --tools claude --yes --existing replace --no-sync
    assert_tree_parity init --yes
}

@test "parity: init keeps existing configs, refuses, and restores like Bash" {
    rm -rf .ai
    mkdir -p subdir
    assert_tree_parity init subdir --no-detect --no-sync
    mkdir -p .ai
    PARITY_CWD=.ai assert_tree_parity init
    printf 'tools:\n  enabled: []\nbackup:\n  retention: typo\n' > .ai/agent_sync.yaml
    assert_tree_parity init --no-detect
    printf 'tools:\n  enabled:\n    - claude\n' > .ai/agent_sync.yaml
    assert_tree_parity init --no-detect --no-sync
    rm -f .ai/agent_sync.yaml
    printf 'tools:\n  enabled: []\n' > agent_sync.yaml
    assert_tree_parity init --no-detect --no-sync
    rm -f agent_sync.yaml
    mkdir -p .ai/agent_sync.yaml
    printf 'keep\n' > .ai/agent_sync.yaml/sentinel
    # The failing write is reported in each engine's own words after these lines.
    assert_parity_head 7 init --no-detect
    [ -f .ai/agent_sync.yaml/sentinel ]
    [ ! -e .ai/src ]
    [ ! -e .ai/.template-manifest ]
}
```

Run: `bats --tap -f 'parity: init' tests/native_parity.bats`
Expected: `ok 1` and `ok 2` (the native side still runs Bash).

- [ ] **Step 2: Write the failing tests**

In `src/catalog.rs`, add to `a_base_payload_is_found_by_slug_and_resource`:

```rust
        assert_eq!(base_payloads("settings", "claude").len(), 1);
        assert_eq!(base_payloads("hooks", "code").len(), 0);
        assert!(CI_GITHUB_WORKFLOW.contains("AGENTSYNC_VERSION=__AGENTSYNC_VERSION__ bash"));
```

Create `src/cli/init.rs` with its tests module only, and add `pub mod init;` to `src/cli/mod.rs` after `pub mod enable;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::template_manifest::REL;

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for (rel, text) in files {
            let path = Path::new(&root).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    struct Outcome {
        status: u8,
        out: String,
        err: String,
        synced: Vec<String>,
        asked: Vec<String>,
        picked: Vec<String>,
    }

    struct Script {
        interactive: bool,
        confirms: Vec<bool>,
        picks: Vec<Result<Vec<String>, Cancelled>>,
        sync_status: u8,
    }

    fn quiet() -> Script {
        Script {
            interactive: false,
            confirms: Vec::new(),
            picks: Vec::new(),
            sync_status: 0,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn call(cwd: &str, args: &[&str], script: Script) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut synced = Vec::new();
        let mut asked = Vec::new();
        let mut picked = Vec::new();
        let mut confirms: VecDeque<bool> = script.confirms.into_iter().collect();
        let mut picks: VecDeque<Result<Vec<String>, Cancelled>> =
            script.picks.into_iter().collect();
        let status_code = script.sync_status;
        let mut confirm = |question: &str, _default: bool| {
            asked.push(question.to_string());
            confirms.pop_front().unwrap_or(true)
        };
        let mut multiselect = |title: &str, _options: &[String], preselected: &[String]| {
            picked.push(title.to_string());
            picks
                .pop_front()
                .unwrap_or_else(|| Ok(preselected.to_vec()))
        };
        let mut sync = |root: &str| {
            synced.push(root.to_string());
            status_code
        };
        let mut env = Env {
            version: "9.9.9",
            cwd: cwd.to_string(),
            config_path: None,
            backup_limit: None,
            backup_max_age: None,
            interactive: script.interactive,
            confirm: &mut confirm,
            multiselect: &mut multiselect,
            sync: &mut sync,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = init(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            synced,
            asked,
            picked,
        }
    }

    fn tree(root: &str) -> Vec<String> {
        let mut files = Vec::new();
        files_below(Path::new(root), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .filter(|rel| !rel.starts_with(".ai/backups/"))
            .collect();
        rels.sort();
        rels
    }

    fn backups(root: &str) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(Path::new(root).join(".ai/backups"))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    const PLAN_NONE: &str = "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules, skills, commands, subagents\n  Tools:    (none — opt in later via 'agentsync enable')\n\n";
    const SUMMARY_FULL: &str = "\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/AGENTS.md      — agent identity\n   Created .ai/src/rules/          — 3 rule(s)\n   Created .ai/src/skills/         — 7 skill(s)\n   Created .ai/src/commands/       — 2 command(s)\n   Created .ai/src/agents/         — 1 subagent(s)\n";
    const NEXT_NO_TOOLS: &str = "\n   No tools enabled. Run 'agentsync enable <slug>' to opt in.\n\nDone!\n\nNext steps:\n  1. Edit .ai/src/AGENTS.md — customize your agent's identity\n  2. Run agentsync generate    — print an AI prompt to tailor .ai/src/ to your codebase\n  3. Run agentsync list        — browse all available tools\n  4. Run agentsync enable <slug> — opt in to tools you use\n  5. Run agentsync sync        — distribute to enabled tools\n\nCustomize:\n  • agentsync add mcp <server>            — configure shared MCP servers\n  • agentsync customize <tool> <resource> — override settings/hooks per tool\n\n";

    fn backup_line(root: &str) -> String {
        let snapshot = backups(root).pop().expect("one snapshot");
        format!(
            "Backup: .ai/backups/{}\n\n",
            snapshot.file_name().unwrap().to_string_lossy()
        )
    }

    #[test]
    fn arguments_and_validation_are_refused_like_bash() {
        let (_dir, root) = project(&[]);
        let help = call(&root, &["--help"], quiet());
        assert_eq!(help.status, 0);
        assert!(
            help.out.starts_with(
                "Usage: agentsync init [<dir>] [OPTIONS]\n\nScaffold .ai/ in a project."
            )
        );
        assert!(
            help.out.ends_with(
                "  agentsync init --dry-run                 # preview without writing\n"
            )
        );
        assert_eq!(help.out.lines().count(), 45);
        let cases: [(&[&str], u8, &str); 9] = [
            (
                &["--bogus"],
                1,
                "Error: Unknown flag: --bogus\nRun agentsync init --help for usage.\n",
            ),
            (&["a", "b"], 1, "Error: Unexpected argument: b\n"),
            (&["--tools"], 1, "Error: --tools requires a value\n"),
            (
                &["--outputs", "bogus"],
                1,
                "Error: --outputs must be 'committed' or 'local' (got 'bogus')\n",
            ),
            (
                &["--existing", "bogus"],
                1,
                "Error: --existing must be 'adopt' or 'replace' (got 'bogus')\n",
            ),
            (
                &["--ci", "gitlab"],
                1,
                "Error: --ci only supports 'github' (got 'gitlab')\n",
            ),
            (
                &["missing-dir"],
                1,
                "Error: Directory not found: missing-dir\n",
            ),
            (
                &["--content", "bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
            (
                &["--tools", "claude", "--content", "agents,bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
        ];
        for (args, status, err) in cases {
            let run = call(&root, args, quiet());
            assert_eq!(
                (run.status, run.out.as_str(), run.err.as_str()),
                (status, "", err),
                "{args:?}"
            );
        }
        assert!(!Path::new(&root).join(".ai").exists());

        std::fs::create_dir_all(Path::new(&root).join(".ai")).unwrap();
        let inside = call(&format!("{root}/.ai"), &[], quiet());
        assert_eq!(
            (inside.status, inside.err),
            (
                2,
                format!(
                    "Error: Cannot init inside the .ai/ directory: {root}/.ai\nRun agentsync init from the project root (the parent of .ai/):\n  cd \"{root}\" && agentsync init\n"
                )
            )
        );
    }

    #[test]
    fn a_plain_init_scaffolds_backs_up_and_skips_a_second_run_like_bash() {
        let (_dir, root) = project(&[]);
        let run = call(&root, &["--no-detect"], quiet());
        assert_eq!(run.status, 0);
        assert_eq!(run.err, "");
        assert_eq!(
            run.out,
            format!(
                "{}Initializing AgentSync in {root}\n\n{SUMMARY_FULL}{NEXT_NO_TOOLS}{}",
                PLAN_NONE.replace("{root}", &root),
                backup_line(&root)
            )
        );
        assert!(run.synced.is_empty());
        let files = tree(&root);
        assert_eq!(files.len(), 21);
        assert!(files.contains(&".ai/.template-manifest".to_string()));
        assert!(files.contains(&".ai/src/skills/humanizer/scripts/strip-ai-chars.sh".to_string()));
        assert!(!Path::new(&root).join(".ai/src/tools").exists());
        let config = std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(config.starts_with("# AgentSync — Project Configuration\n# All keys are optional — remove any that you leave at the default.\n\nagentsync_version: \"9.9.9\"\nformat: 2\n\n# Tools:"));
        assert!(config.contains("\ntools:\n  enabled: []\n\n# Source paths"));
        assert!(config.ends_with("outputs: committed\n\n# .gitignore management (false leaves the managed block untouched).\ngitignore:\n  update: true\n"));
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(REL))
                .unwrap()
                .lines()
                .count(),
            19
        );
        let snapshot = backups(&root).pop().unwrap();
        assert_eq!(
            std::fs::read_to_string(snapshot.join("targets.tsv")).unwrap(),
            "missing\t.ai/src\nmissing\t.ai/agent_sync.yaml\nmissing\t.ai/.template-manifest\n"
        );
        assert!(snapshot.join("after.tsv").is_file());
        assert!(
            std::fs::read_to_string(snapshot.join("metadata"))
                .unwrap()
                .contains("operation=init\n")
        );

        let again = call(&root, &["--tools", "claude", "--no-sync"], quiet());
        assert_eq!(
            (again.status, again.out),
            (
                0,
                format!(
                    "Warning: .ai/src/ already exists in {root}\nSkipping init to avoid overwriting your content.\n\nRun agentsync sync to synchronize.\n"
                )
            )
        );
    }

    #[test]
    fn markers_flags_and_content_shape_the_plan_like_bash() {
        let (_dir, root) = project(&[
            (".claude/x", ""),
            (".cursor/x", ""),
            (".github/instructions/x", ""),
            (".gemini/x", ""),
            (".codex/x", ""),
            (".kimi-code/x", ""),
            (".opencode/x", ""),
            (".windsurf/x", ""),
            (".junie/x", ""),
            (".clinerules", ""),
            (".amazonq/x", ""),
            (".zed/x", ""),
            (".agents/rules/x", ""),
        ]);
        let all = call(&root, &["--dry-run"], quiet());
        assert_eq!(
            all.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules, skills, commands, subagents\n  Tools:    claude, cursor, copilot, gemini, codex, kimi, opencode, windsurf, junie, cline, amazonq, zed, antigravity (detect)\n  settings: claude.json, gemini.json, codex.toml, opencode.json, zed.json\n  hooks:    cursor.json, copilot.json, codex.json, opencode.ts, windsurf.json\n\nDry run — nothing was written.\n"
            )
        );
        assert!(!Path::new(&root).join(".ai").exists());

        let (_dir, root) = project(&[
            ("AGENTS.md", "# generic\n"),
            ("GEMINI.md", ""),
            (".rules", ""),
            ("opencode.json", ""),
        ]);
        let files = call(&root, &["--dry-run"], quiet());
        assert!(files.out.contains("  Tools:    gemini, opencode, zed (detect)\n  settings: gemini.json, opencode.json, zed.json\n  hooks:    opencode.ts\n"));

        let (_dir, root) = project(&[(".cursor/x", "")]);
        let mixed = call(
            &root,
            &[
                "--dry-run",
                "--tools",
                "claude, cursor,claude",
                "--content",
                " rules , skills ",
            ],
            quiet(),
        );
        assert!(mixed.out.contains("  Content:  rules, skills\n  Tools:    claude, cursor (mixed)\n  settings: claude.json\n  hooks:    cursor.json\n"));
        let no_templates = call(
            &root,
            &[
                "--dry-run",
                "--no-templates",
                "--no-detect",
                "--content",
                "agents,rules",
            ],
            quiet(),
        );
        assert_eq!(
            no_templates.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules (no starter templates)\n  Tools:    (none — opt in later via 'agentsync enable')\n\nDry run — nothing was written.\n"
            )
        );
        let empty = call(
            &root,
            &["--dry-run", "--no-detect", "--content", ","],
            quiet(),
        );
        assert!(empty.out.contains("  Content:  (none)\n"));
        let kimi = call(
            &root,
            &["--dry-run", "--no-detect", "--tools", "kimi"],
            quiet(),
        );
        assert!(kimi.out.contains("  Tools:    kimi (flag)\n  No payloads to scaffold — tools will use base templates at sync time.\n"));
    }

    #[test]
    fn payloads_config_and_summary_follow_the_selected_tools() {
        let (_dir, root) = project(&[]);
        let run = call(
            &root,
            &[
                "--tools",
                "claude,cursor",
                "--content",
                "agents,rules",
                "--no-sync",
            ],
            quiet(),
        );
        assert_eq!(
            run.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules\n  Tools:    claude, cursor (flag)\n  settings: claude.json\n  hooks:    cursor.json\n\nInitializing AgentSync in {root}\n\n\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/AGENTS.md      — agent identity\n   Created .ai/src/rules/          — 3 rule(s)\n   Created .ai/src/tools/claude/settings.json\n   Created .ai/src/tools/cursor/hooks.json\n\n   Enabled 2 tool(s): claude, cursor (from --tools)\n\nDone!\n\nNext steps:\n  1. Edit .ai/src/AGENTS.md — customize your agent's identity\n  2. Run agentsync generate    — print an AI prompt to tailor .ai/src/ to your codebase\n  3. Run agentsync list        — browse all available tools\n  4. Run agentsync enable <slug> — add more tools\n  5. Run agentsync sync        — distribute to enabled tools\n\nCustomize:\n  • agentsync add mcp <server>            — configure shared MCP servers\n  • agentsync customize <tool> <resource> — override settings/hooks per tool\n\n{}",
                backup_line(&root)
            )
        );
        assert_eq!(
            tree(&root),
            [
                ".ai/.template-manifest",
                ".ai/agent_sync.yaml",
                ".ai/src/AGENTS.md",
                ".ai/src/rules/comments.md",
                ".ai/src/rules/core.md",
                ".ai/src/rules/git.md",
                ".ai/src/tools/claude/settings.json",
                ".ai/src/tools/cursor/hooks.json"
            ]
        );
        let config = std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(config.contains("\ntools:\n  enabled:\n    - claude\n    - cursor\n\n"));
        let snapshot = backups(&root).pop().unwrap();
        let targets = std::fs::read_to_string(snapshot.join("targets.tsv")).unwrap();
        assert!(targets.starts_with("missing\t.ai/src\nmissing\t.ai/agent_sync.yaml\nmissing\t.ai/.template-manifest\nmissing\tCLAUDE.md\n"));
        assert!(targets.contains("missing\t.cursor/hooks.json\n"));

        let (_dir, root) = project(&[]);
        let empty = call(
            &root,
            &[
                "--no-templates",
                "--no-detect",
                "--content",
                "rules,agents",
                "--outputs=local",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(empty.out.contains("\n   Created .ai/agent_sync.yaml     — project config (outputs: local — every clone runs agentsync sync)\n   Created .ai/src/AGENTS.md      — (empty)\n   Created .ai/src/rules/          — (empty)\n\n   No tools enabled."));
        assert_eq!(
            std::fs::metadata(Path::new(&root).join(".ai/src/AGENTS.md"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::read_dir(Path::new(&root).join(".ai/src/rules"))
                .unwrap()
                .count(),
            0
        );
        assert!(!Path::new(&root).join(REL).exists());
        let (_dir, root) = project(&[]);
        let rules_only = call(
            &root,
            &[
                "--no-templates",
                "--no-detect",
                "--content",
                "rules",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(
            rules_only
                .out
                .contains("  Content:  rules (no starter templates)\n")
        );
        assert!(rules_only.out.contains("\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/rules/          — (empty)\n\n   No tools enabled."));
        assert!(
            rules_only
                .out
                .contains("Next steps:\n  1. Run agentsync generate")
        );
        assert!(!Path::new(&root).join(".ai/src/AGENTS.md").exists());
    }

    #[test]
    fn existing_outputs_are_adopted_first_wins_or_replaced_like_bash() {
        let (_dir, root) = project(&[
            ("CLAUDE.md", "# Hand-written team rules\n"),
            (".claude/rules/legacy.md", "# Legacy rule\n"),
            (".claude/settings.json", "{\"settings\": true}\n"),
        ]);
        let run = call(&root, &["--tools", "claude", "--yes", "--no-sync"], quiet());
        assert_eq!(run.status, 0);
        assert!(run.out.contains(&format!(
            "Initializing AgentSync in {root}\n\n\n   Adopted .claude/rules/legacy.md → .ai/src/rules/legacy.md\n   Adopted .claude/settings.json → .ai/src/tools/claude/settings.json\n   Adopted CLAUDE.md → .ai/src/AGENTS.md\n\n\n   Created .ai/agent_sync.yaml"
        )));
        assert!(
            run.out
                .contains("   Created .ai/src/rules/          — 4 rule(s)\n")
        );
        assert!(
            run.out
                .contains("   Enabled 1 tool(s): claude (auto-detect + --tools)\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/tools/claude/settings.json"))
                .unwrap(),
            "{\"settings\": true}\n"
        );
        let snapshot = backups(&root).pop().unwrap();
        assert!(snapshot.join("files/CLAUDE.md").is_file());
        assert!(snapshot.join("files/.claude/rules/legacy.md").is_file());

        let (_dir, root) = project(&[
            ("CLAUDE.md", "# From CLAUDE\n"),
            ("AGENTS.md", "# From AGENTS\n"),
        ]);
        let two = call(
            &root,
            &["--tools", "claude,codex", "--yes", "--no-sync"],
            quiet(),
        );
        assert!(two.out.contains("\n   Adopted AGENTS.md → .ai/src/AGENTS.md\n   Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md\n   Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.\n\n"));
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# From AGENTS\n"
        );

        let (_dir, root) = project(&[
            (".cursor/rules/core.mdc", "body\n"),
            (".cursor/mcp.json", "{}\n"),
        ]);
        let refused = call(&root, &["--tools", "cursor", "--yes", "--no-sync"], quiet());
        assert!(refused.out.contains("\n   Adopted .cursor/mcp.json → .ai/src/tools/cursor/mcp.json\n   Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.\n   Skipped files"));

        let (_dir, root) = project(&[("CLAUDE.md", "# Hand-written team rules\n")]);
        let replaced = call(
            &root,
            &[
                "--tools",
                "claude",
                "--yes",
                "--existing",
                "replace",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(!replaced.out.contains("Adopted"));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md"))
                .unwrap()
                .starts_with("# ")
        );
        assert_ne!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
    }

    #[test]
    fn the_ci_gate_is_written_once_with_the_pinned_version() {
        let (_dir, root) = project(&[]);
        let run = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(run.out.contains(&format!("Initializing AgentSync in {root}\n\n   Created .github/workflows/agentsync-check.yml — CI gate (agentsync check)\n\n   Created .ai/agent_sync.yaml")));
        let workflow = Path::new(&root).join(".github/workflows/agentsync-check.yml");
        let text = std::fs::read_to_string(&workflow).unwrap();
        assert!(text.contains("AGENTSYNC_VERSION=9.9.9 bash"));
        assert!(
            text.contains(
                "https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh"
            )
        );
        assert!(!text.contains("__AGENTSYNC_"));
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&workflow).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let (_dir, root) = project(&[(".github/workflows/agentsync-check.yml", "name: mine\n")]);
        let kept = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(
            kept.out
                .contains("\n   Kept .github/workflows/agentsync-check.yml (already exists)\n\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".github/workflows/agentsync-check.yml"))
                .unwrap(),
            "name: mine\n"
        );
        let none = call(
            &root,
            &["--dry-run", "--tools", "claude", "--ci", "github"],
            quiet(),
        );
        assert!(!none.out.contains("workflows"));
    }

    #[test]
    fn the_first_sync_runs_for_enabled_tools_and_reports_committed_mode() {
        let (_dir, root) = project(&[]);
        let run = call(&root, &["--tools", "claude"], quiet());
        assert_eq!(run.synced, std::slice::from_ref(&root));
        assert!(
            run.out
                .contains("  5. Re-run agentsync sync     — after every change to .ai/src/\n")
        );
        assert!(run.out.ends_with(&format!("{}Running the first sync\n\nCommit .ai/ and the generated files — teammates then need only git pull.\n\n", backup_line(&root))));

        let (_dir, root) = project(&[]);
        let local = call(&root, &["--tools", "claude", "--outputs", "local"], quiet());
        assert!(local.out.ends_with("Running the first sync\n\n"));

        let (_dir, root) = project(&[]);
        let failed = call(
            &root,
            &["--tools", "claude"],
            Script {
                sync_status: 1,
                ..quiet()
            },
        );
        assert_eq!(
            (failed.status, failed.err.as_str()),
            (
                0,
                "Warning: first sync failed — fix the cause and run agentsync sync.\n"
            )
        );
        assert!(failed.out.ends_with("Running the first sync\n\n"));

        let (_dir, root) = project(&[]);
        let none = call(&root, &["--no-detect"], quiet());
        assert!(none.synced.is_empty());
        let (_dir, root) = project(&[]);
        let skipped = call(&root, &["--tools", "claude", "--no-sync"], quiet());
        assert!(skipped.synced.is_empty());
        assert!(
            skipped
                .out
                .contains("  5. Run agentsync sync        — distribute to enabled tools\n")
        );
    }

    #[test]
    fn a_scaffold_failure_restores_the_snapshot_like_bash() {
        let (_dir, root) = project(&[(".ai/agent_sync.yaml/sentinel", "keep\n")]);
        let run = call(&root, &["--no-detect"], quiet());
        assert_eq!(run.status, 1);
        assert_eq!(
            run.out,
            format!(
                "{}Initializing AgentSync in {root}\n\n",
                PLAN_NONE.replace("{root}", &root)
            )
        );
        let snapshot = backups(&root).pop().unwrap();
        assert_eq!(
            run.err,
            format!(
                "{root}/.ai/agent_sync.yaml: Is a directory (os error 21)\nWarning: Init failed; restoring pre-init state...\nRestored pre-init state from .ai/backups/{}\n",
                snapshot.file_name().unwrap().to_string_lossy()
            )
        );
        assert_eq!(tree(&root), [".ai/agent_sync.yaml/sentinel"]);
        assert!(snapshot.join("after.tsv").is_file());
    }

    #[test]
    fn preexisting_configs_are_kept_and_validated_like_bash() {
        let (_dir, root) =
            project(&[(".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n")]);
        let run = call(&root, &["--no-detect", "--no-sync"], quiet());
        assert_eq!(run.status, 0);
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap(),
            "tools:\n  enabled:\n    - claude\n"
        );

        let (_dir, root) = project(&[("agent_sync.yaml", "tools:\n  enabled: []\n")]);
        let root_config = call(&root, &["--no-detect", "--no-sync"], quiet());
        assert_eq!(root_config.status, 0);
        assert!(!Path::new(&root).join(".ai/agent_sync.yaml").exists());

        let (_dir, root) = project(&[(".ai/agent_sync.yaml", "backup:\n  retention: typo\n")]);
        let typo = call(&root, &["--no-detect"], quiet());
        assert_eq!(
            (typo.status, typo.err),
            (
                1,
                format!(
                    "Error: Invalid backup.retention 'typo' in {root}/.ai/agent_sync.yaml; expected bounded or preserve\n"
                )
            )
        );
        assert!(!Path::new(&root).join(".ai/src").exists());
    }

    #[test]
    fn the_wizard_picks_tools_content_outputs_existing_and_ci_like_bash() {
        let (_dir, root) = project(&[
            (".github/x", ""),
            ("CLAUDE.md", "# Hand-written\n"),
            (".claude/rules/legacy.md", "# Legacy\n"),
        ]);
        let run = call(
            &root,
            &["--no-sync"],
            Script {
                interactive: true,
                confirms: vec![false, true, true, true],
                picks: vec![
                    Ok(strings(&["claude", "cursor"])),
                    Ok(strings(&["agents", "rules"])),
                ],
                sync_status: 0,
            },
        );
        assert_eq!(run.status, 0);
        assert_eq!(
            run.picked,
            ["Tools to enable (detected: claude):", "Content sections:"]
        );
        assert_eq!(
            run.asked,
            [
                "Commit generated files?",
                "Copy them into .ai/src/ first, so sync reproduces them?",
                "Proceed?"
            ]
        );
        assert!(run.out.starts_with(&format!(
            "\nAgentSync init — {root}\n\n\n\nGenerated files (CLAUDE.md, .claude/, .cursor/, …) can be committed, so\nteammates get current rules from git pull and never run agentsync.\n\nFound 2 existing tool config file(s) — the first sync regenerates these paths:\n   .claude/rules/legacy.md\n   CLAUDE.md\n\nPlan:\n  Target:   {root}/.ai/\n  Content:  agents, rules\n  Tools:    claude, cursor (interactive)\n  settings: claude.json\n  hooks:    cursor.json\n\n\nInitializing AgentSync in {root}\n\n\n   Adopted .claude/rules/legacy.md → .ai/src/rules/legacy.md\n   Adopted CLAUDE.md → .ai/src/AGENTS.md\n\n"
        )));
        assert!(
            run.out
                .contains("(outputs: local — every clone runs agentsync sync)")
        );
        assert!(
            run.out
                .contains("   Enabled 2 tool(s): claude, cursor (selected)\n")
        );
        assert!(!Path::new(&root).join(".github/workflows").exists());

        let (_dir, root) = project(&[(".github/x", "")]);
        let ci = call(
            &root,
            &["--no-sync"],
            Script {
                interactive: true,
                confirms: vec![true, true, true],
                picks: vec![Ok(strings(&["claude"])), Ok(strings(&["rules"]))],
                sync_status: 0,
            },
        );
        assert_eq!(
            ci.asked,
            [
                "Commit generated files?",
                "Add a GitHub Actions gate that runs 'agentsync check'?",
                "Proceed?"
            ]
        );
        assert!(ci.out.contains("\n\n\nPlan:\n"));
        assert!(
            Path::new(&root)
                .join(".github/workflows/agentsync-check.yml")
                .is_file()
        );

        let (_dir, root) = project(&[]);
        let declined = call(
            &root,
            &[],
            Script {
                interactive: true,
                confirms: vec![true, false],
                picks: Vec::new(),
                sync_status: 0,
            },
        );
        assert_eq!(declined.status, 130);
        assert!(declined.out.ends_with(&format!(
            "{}Cancelled.\n",
            PLAN_NONE.replace("{root}", &root)
        )));
        assert_eq!(
            declined.picked,
            ["Tools to enable (none auto-detected):", "Content sections:"]
        );
        assert!(!Path::new(&root).join(".ai").exists());

        let cancelled = call(
            &root,
            &[],
            Script {
                interactive: true,
                confirms: Vec::new(),
                picks: vec![Err(Cancelled(Vec::new()))],
                sync_status: 0,
            },
        );
        assert_eq!(
            (cancelled.status, cancelled.err.as_str()),
            (130, "Cancelled.\n")
        );
        assert_eq!(cancelled.out, format!("\nAgentSync init — {root}\n\n"));

        let dry = call(
            &root,
            &["--dry-run"],
            Script {
                interactive: true,
                confirms: vec![true],
                picks: vec![Ok(Vec::new()), Ok(strings(&["rules"]))],
                sync_status: 0,
            },
        );
        assert_eq!(dry.status, 0);
        assert!(dry.out.ends_with("  Content:  rules\n  Tools:    (none — opt in later via 'agentsync enable')\n\nDry run — nothing was written.\n"));
        assert_eq!(dry.asked, ["Commit generated files?"]);

        let flagged = call(
            &root,
            &["--yes"],
            Script {
                interactive: true,
                confirms: Vec::new(),
                picks: Vec::new(),
                sync_status: 0,
            },
        );
        assert!(flagged.picked.is_empty());
        assert!(flagged.asked.is_empty());
        assert_eq!(flagged.status, 0);
    }
}
```

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort -u | head -8`
Expected: compile errors naming the missing `init`, `Env`, `Cancelled`, `base_payloads`, `CI_GITHUB_WORKFLOW`, and the module's imports.

- [ ] **Step 3: Write the implementation**

In `src/catalog.rs`, replace `base_payload` and its doc comment with:

```rust
/// First shipped `lib/templates/<resource>/<slug>.*` by name, as the Bash glob picks it.
pub fn base_payload(resource: &str, slug: &str) -> Option<&'static File<'static>> {
    base_payloads(resource, slug).first().copied()
}

/// Every shipped `lib/templates/<resource>/<slug>.*` in byte order, as the
/// `"$templates_dir/$resource/$tool".*` glob lists them.
pub fn base_payloads(resource: &str, slug: &str) -> Vec<&'static File<'static>> {
    let prefix = format!("{slug}.");
    let mut matches: Vec<&'static File<'static>> = files_in(resource)
        .filter(|file| file_name(file).is_some_and(|name| name.starts_with(&prefix)))
        .collect();
    matches.sort_by(|a, b| a.path().cmp(b.path()));
    matches
}

/// `lib/templates/ci/github-agentsync-check.yml`, the gate `init --ci github` writes.
pub const CI_GITHUB_WORKFLOW: &str = include_str!("../lib/templates/ci/github-agentsync-check.yml");
```

In `src/cli/adopt.rs` make `copy_into_source` `pub(crate)`; in `src/cli/refresh.rs` make `write_template` `pub(crate)`.

Prepend to `src/cli/init.rs`:

```rust
//! `agentsync init`: `cmd_init` of `lib/helpers/init.sh`, which scaffolds
//! `.ai/` inside a backup transaction, adopts the tool config a project already
//! has, writes the CI gate, and runs the first sync.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::adopt::{self, Resolver};
use super::customize::put;
use super::refresh::write_template;
use crate::interrupt::{self, Interrupt};
use crate::log::Log;
use crate::paths::{self, Paths};
use crate::project::Project;
use crate::project_config::{self, Selection};
use crate::prompts::Cancelled;
use crate::render::TARGET_KEYS;
use crate::style::Style;
use crate::template_manifest::TemplateManifest;
use crate::tool::Tool;
use crate::{Error, backup, catalog, format_rev, staging, witness};

const CONTENT_DEFAULT: &str = "agents,rules,skills,commands,subagents";
const CONTENT_VALID: [&str; 5] = ["agents", "rules", "skills", "commands", "subagents"];

/// `AGENTSYNC_REPO` of `lib/helpers/update.sh`, which the CI template's install
/// URL names.
const REPO: &str = "yelmuratoff/agent_sync";

/// `_init_detect_enabled_tools`: a tool is detected when any marker exists.
const DETECTORS: [(&str, &[&str]); 13] = [
    ("claude", &[".claude", "CLAUDE.md"]),
    ("cursor", &[".cursor", ".cursorrules"]),
    (
        "copilot",
        &[
            ".github/copilot-instructions.md",
            ".github/instructions",
            ".github/prompts",
        ],
    ),
    ("gemini", &[".gemini", "GEMINI.md"]),
    ("codex", &[".codex"]),
    ("kimi", &[".kimi-code"]),
    (
        "opencode",
        &[".opencode", "opencode.json", "opencode.jsonc"],
    ),
    ("windsurf", &[".windsurf", ".windsurfrules"]),
    ("junie", &[".junie"]),
    ("cline", &[".clinerules"]),
    ("amazonq", &[".amazonq"]),
    ("zed", &[".zed", ".rules"]),
    ("antigravity", &[".agents/rules", ".agents/workflows"]),
];

const HELP: &str = "Usage: agentsync init [<dir>] [OPTIONS]

Scaffold .ai/ in a project. Minimal by default — only tools you opt in to
get per-tool payload scaffolding (settings/mcp/hooks).

Before writing, init snapshots its .ai/ paths and the selected tools' existing
destinations under .ai/backups/ so a partial setup can be restored safely.

In a terminal, `init` opens an interactive wizard that lets you pick tools
and content sections. In non-TTY environments (CI, scripts), it runs silently
with auto-detected defaults. Pass --yes or any of --tools/--content/--no-detect
/--no-templates to skip the wizard.

Options:
  --tools <csv>        Enable these tools (e.g. claude,cursor). Unions with
                       auto-detection unless --no-detect is passed.
  --content <csv>      Which source sections to scaffold. Valid tokens:
                       agents, rules, skills, commands, subagents.
                       Default: all of them.
  --no-detect          Skip filesystem marker auto-detection (tools only).
  --outputs <mode>     Where generated tool files live. `committed` (default)
                       keeps them and .ai/.sync-manifest in git so teammates
                       need only `git pull`; `local` gitignores both and every
                       clone runs `agentsync sync`.
  --existing <action>  What to do with tool config the project already has:
                       `adopt` (default) copies it into .ai/src/ so the first
                       sync reproduces it; `replace` regenerates from the
                       shipped templates.
  --ci <provider>      Write a CI gate that runs `agentsync check`. Only
                       `github` is supported; an existing workflow is kept.
  --no-sync            Skip the first `agentsync sync` at the end.
  --no-templates       Create selected content paths without copying shipped
                       starter files. AGENTS.md is empty when agents is selected.
  -y, --yes            Skip all prompts, accept defaults.
  --dry-run            Show what would be created; don't write anything.
  -h, --help           Show this help.

Examples:
  agentsync init                           # interactive wizard (TTY)
  agentsync init --yes                     # auto-detect + defaults, no prompt
  agentsync init --tools claude            # Claude only, no detection union
  agentsync init --tools claude,cursor --content agents,rules
  agentsync init --no-detect               # no tool auto-detection; pick tools later
  agentsync init --no-templates --no-detect  # empty .ai/src/ layout, no starters
  agentsync init --dry-run                 # preview without writing
";

/// `prompt_multiselect`: title, options, preselected.
pub type Picker<'a> =
    &'a mut dyn FnMut(&str, &[String], &[String]) -> Result<Vec<String>, Cancelled>;

/// What `init` takes from the process and the terminal.
pub struct Env<'a> {
    pub version: &'a str,
    /// `$(pwd)`, spelled logically.
    pub cwd: String,
    pub config_path: Option<String>,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
    /// `is_tty`: stdin and stdout are both terminals.
    pub interactive: bool,
    /// `prompt_confirm`.
    pub confirm: &'a mut dyn FnMut(&str, bool) -> bool,
    pub multiselect: Picker<'a>,
    /// `bash "$system_dir/sync.sh"` for a root: its exit status.
    pub sync: &'a mut dyn FnMut(&str) -> u8,
}

struct Options {
    target: Option<String>,
    tools: Option<String>,
    content: Option<String>,
    no_detect: bool,
    outputs: String,
    existing: String,
    ci: String,
    run_sync: bool,
    no_templates: bool,
    assume_yes: bool,
    dry_run: bool,
}

struct Run<'a, 'b> {
    style: &'a Style,
    env: &'a mut Env<'b>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Run<'_, '_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn tell(&mut self, text: &str) -> Result<(), Error> {
        put(self.err, text.as_bytes())
    }
}

pub fn init(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut run = Run {
        style,
        env,
        out,
        err,
    };
    let options = match parse_args(args, &mut run)? {
        Ok(options) => options,
        Err(status) => return Ok(status),
    };

    if !matches!(options.outputs.as_str(), "committed" | "local") {
        run.tell(&format!(
            "{}: --outputs must be 'committed' or 'local' (got '{}')\n",
            style.red("Error"),
            options.outputs
        ))?;
        return Ok(1);
    }
    if !matches!(options.existing.as_str(), "adopt" | "replace") {
        run.tell(&format!(
            "{}: --existing must be 'adopt' or 'replace' (got '{}')\n",
            style.red("Error"),
            options.existing
        ))?;
        return Ok(1);
    }
    if !matches!(options.ci.as_str(), "" | "github") {
        run.tell(&format!(
            "{}: --ci only supports 'github' (got '{}')\n",
            style.red("Error"),
            options.ci
        ))?;
        return Ok(1);
    }

    let requested = options.target.clone().unwrap_or_else(|| ".".to_string());
    let target = if requested.starts_with('/') {
        paths::normalize(&requested)
    } else {
        paths::normalize(&format!("{}/{requested}", run.env.cwd))
    };
    if !Path::new(&target).is_dir() {
        run.tell(&format!(
            "{}: Directory not found: {requested}\n",
            style.red("Error")
        ))?;
        return Ok(1);
    }

    if let Some(project_root) = paths::ai_dir_enclosing_root(&target) {
        run.tell(&format!(
            "{}: Cannot init inside the .ai/ directory: {target}\nRun agentsync init from the project root (the parent of .ai/):\n  cd \"{project_root}\" && agentsync init\n",
            style.red("Error")
        ))?;
        return Ok(2);
    }

    let ai_dir = format!("{target}/.ai");

    let is_file = |path: &str| Path::new(path).is_file();
    let config = match project_config::select(&target, run.env.config_path.as_deref(), &is_file) {
        Selection::Found(path) => {
            let text = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
            Some((path, String::from_utf8_lossy(&text).into_owned()))
        }
        Selection::None => None,
        Selection::Missing(path) => {
            run.tell(&format!(
                "Error: {}\n",
                project_config::missing_message(&path)
            ))?;
            return Ok(1);
        }
    };
    let retention = match backup::configure(
        config
            .as_ref()
            .map(|(path, text)| (path.as_str(), text.as_str())),
        run.env.backup_limit.as_deref(),
        run.env.backup_max_age.as_deref(),
    ) {
        Ok(retention) => retention,
        Err(e) => {
            report_backup_error(&mut run, &e)?;
            return Ok(1);
        }
    };

    if Path::new(&ai_dir).join("src").is_dir() {
        run.say(&format!(
            "{}: .ai/src/ already exists in {target}\nSkipping init to avoid overwriting your content.\n\nRun {} to synchronize.\n",
            style.yellow("Warning"),
            style.cyan("agentsync sync")
        ))?;
        return Ok(0);
    }

    let mut content_list = normalize_csv(
        options
            .content
            .as_deref()
            .filter(|c| !c.is_empty())
            .unwrap_or(CONTENT_DEFAULT),
    );
    for token in &content_list {
        if !CONTENT_VALID.contains(&token.as_str()) {
            run.tell(&format!(
                "{}: Unknown --content section: {token}\nValid sections: agents rules skills commands subagents\n",
                style.red("Error")
            ))?;
            return Ok(1);
        }
    }

    let tools_from_flag = normalize_csv(options.tools.as_deref().unwrap_or(""));
    let tools_from_detect = if options.no_detect {
        Vec::new()
    } else {
        detect_tools(&target)
    };
    let mut tool_list = merge_lists(&tools_from_flag, &tools_from_detect);
    let mut detect_source = match (tools_from_flag.is_empty(), tools_from_detect.is_empty()) {
        (false, false) => "mixed",
        (false, true) => "flag",
        (true, false) => "detect",
        (true, true) => "none",
    };

    let mut outputs = options.outputs.clone();
    let mut existing_action = options.existing.clone();
    let mut ci = options.ci.clone();

    let interactive = run.env.interactive
        && !options.assume_yes
        && options.tools.is_none()
        && options.content.is_none()
        && !options.no_templates;

    if interactive {
        run.say(&format!(
            "\n{} — {}\n\n",
            style.bold("AgentSync init"),
            style.dim(&target)
        ))?;
        let available = catalog::base_tools();
        if !available.is_empty() {
            let title = if tool_list.is_empty() {
                format!("Tools to enable {}", style.dim("(none auto-detected):"))
            } else {
                format!(
                    "Tools to enable {}",
                    style.dim(&format!("(detected: {}):", tool_list.join(",")))
                )
            };
            match (run.env.multiselect)(&title, &available, &tool_list) {
                Ok(picked) => tool_list = picked,
                Err(Cancelled(_)) => {
                    run.tell(&format!("{}\n", style.yellow("Cancelled.")))?;
                    return Ok(130);
                }
            }
            detect_source = "interactive";
            run.say("\n")?;
        }
        let sections: Vec<String> = CONTENT_VALID.iter().map(|s| s.to_string()).collect();
        match (run.env.multiselect)("Content sections:", &sections, &content_list) {
            Ok(picked) => content_list = picked,
            Err(Cancelled(_)) => {
                run.tell(&format!("{}\n", style.yellow("Cancelled.")))?;
                return Ok(130);
            }
        }
        run.say("\n")?;
        run.say(&format!(
            "{}\n{}\n",
            style.dim("Generated files (CLAUDE.md, .claude/, .cursor/, …) can be committed, so"),
            style.dim("teammates get current rules from git pull and never run agentsync.")
        ))?;
        outputs = if (run.env.confirm)("Commit generated files?", true) {
            "committed".to_string()
        } else {
            "local".to_string()
        };
        run.say("\n")?;
    }

    let project = Project::at(&target)?;
    let existing = if tool_list.is_empty() {
        Vec::new()
    } else {
        existing_dest_files(&project, &target, &tool_list)?
    };

    if interactive && !existing.is_empty() {
        let mut text = format!(
            "{} {}\n",
            style.yellow(&format!(
                "Found {} existing tool config file(s)",
                existing.len()
            )),
            style.dim("— the first sync regenerates these paths:")
        );
        for line in existing.iter().take(10) {
            text.push_str(&format!("   {line}\n"));
        }
        if existing.len() > 10 {
            text.push_str(&format!(
                "   {}\n",
                style.dim(&format!("… and {} more", existing.len() - 10))
            ));
        }
        run.say(&text)?;
        existing_action = if (run.env.confirm)(
            "Copy them into .ai/src/ first, so sync reproduces them?",
            true,
        ) {
            "adopt".to_string()
        } else {
            "replace".to_string()
        };
        run.say("\n")?;
    }

    if interactive
        && ci.is_empty()
        && outputs == "committed"
        && Path::new(&target).join(".github").is_dir()
    {
        if (run.env.confirm)(
            "Add a GitHub Actions gate that runs 'agentsync check'?",
            true,
        ) {
            ci = "github".to_string();
        }
        run.say("\n")?;
    }

    run.say(&plan(
        style,
        &target,
        &tool_list,
        &content_list,
        detect_source,
        options.no_templates,
    ))?;

    if options.dry_run {
        run.say(&format!(
            "{}\n",
            style.dim("Dry run — nothing was written.")
        ))?;
        return Ok(0);
    }

    if interactive {
        if !(run.env.confirm)("Proceed?", true) {
            run.say(&format!("{}\n", style.yellow("Cancelled.")))?;
            return Ok(130);
        }
        run.say("\n")?;
    }

    run.say(&format!(
        "{} in {}\n\n",
        style.bold("Initializing AgentSync"),
        style.cyan(&target)
    ))?;

    let targets = backup_targets(&mut run, &project, &target, &tool_list)?;
    let backup_path = match backup::create(&target, "init", &targets, retention) {
        Ok(path) => path,
        Err(e) => {
            report_backup_error(&mut run, &e)?;
            run.tell(&format!(
                "{}: Could not back up init targets; no project files were changed.\n",
                style.red("Error")
            ))?;
            return Ok(1);
        }
    };
    let shown_backup = backup_path
        .strip_prefix(&format!("{target}/"))
        .unwrap_or(&backup_path)
        .to_string();

    let mut interrupt = Interrupt::arm();
    let scaffold = scaffold(
        &mut run,
        &mut interrupt,
        Scaffold {
            target: &target,
            ai_dir: &ai_dir,
            content: &content_list,
            tools: &tool_list,
            no_templates: options.no_templates,
            outputs: &outputs,
            adopt: existing_action == "adopt",
            existing: &existing,
            ci_github: ci == "github",
            detect_source,
            run_sync: options.run_sync,
            shown_backup: &shown_backup,
        },
    );
    let status = match scaffold {
        Ok(()) => 0,
        Err(failure) => {
            let status = match &failure {
                Failure::Io(e) => {
                    run.tell(&format!("{e}\n"))?;
                    1
                }
                Failure::Signal(sig) => interrupt::status(*sig),
            };
            run.tell(&format!(
                "{}: Init failed; restoring pre-init state...\n",
                style.yellow("Warning")
            ))?;
            match backup::restore(&target, &backup_path) {
                Ok(()) => {
                    if let Err(reason) = witness::seal(&target, &backup_path) {
                        run.tell(&format!(
                            "{}: Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.\n",
                            style.yellow("Warning"),
                            paths::leaf(&backup_path)
                        ))?;
                    }
                    run.tell(&format!("Restored pre-init state from {shown_backup}\n"))?;
                    prune(&mut run, &target, retention)?;
                }
                Err(e) => {
                    report_backup_error(&mut run, &e)?;
                    run.tell(&format!(
                        "{}: Automatic restore failed. Backup retained at {shown_backup}\n",
                        style.red("Error")
                    ))?;
                }
            }
            if let Failure::Signal(sig) = failure {
                interrupt.resend(sig);
            }
            return Ok(status);
        }
    };
    drop(interrupt);

    prune(&mut run, &target, retention)?;
    if let Err(reason) = witness::seal(&target, &backup_path) {
        run.tell(&format!(
            "{}: Could not record the post-init state ({reason}); rolling back backup {} cannot detect later changes.\n",
            style.yellow("Warning"),
            paths::leaf(&backup_path)
        ))?;
    }

    if options.run_sync && !tool_list.is_empty() {
        run.say(&format!("{}\n\n", style.bold("Running the first sync")))?;
        if (run.env.sync)(&target) != 0 {
            run.tell(&format!(
                "{}: first sync failed — fix the cause and run {}.\n",
                style.yellow("Warning"),
                style.cyan("agentsync sync")
            ))?;
            return Ok(0);
        }
        if outputs == "committed" {
            run.say(&format!(
                "{} {}\n\n",
                style.bold("Commit .ai/ and the generated files"),
                style.dim("— teammates then need only git pull.")
            ))?;
        }
    }
    Ok(status)
}

fn parse_args(args: &[String], run: &mut Run) -> Result<Result<Options, u8>, Error> {
    let style = run.style;
    let mut options = Options {
        target: None,
        tools: None,
        content: None,
        no_detect: false,
        outputs: "committed".to_string(),
        existing: "adopt".to_string(),
        ci: String::new(),
        run_sync: true,
        no_templates: false,
        assume_yes: false,
        dry_run: false,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut valued = |flag: &str, run: &mut Run| -> Result<Result<String, u8>, Error> {
            match rest.next() {
                Some(value) => Ok(Ok(value.clone())),
                None => {
                    run.tell(&format!(
                        "{}: {flag} requires a value\n",
                        style.red("Error")
                    ))?;
                    Ok(Err(1))
                }
            }
        };
        match arg.as_str() {
            "--tools" => match valued("--tools", run)? {
                Ok(value) => options.tools = Some(value),
                Err(status) => return Ok(Err(status)),
            },
            "--content" => match valued("--content", run)? {
                Ok(value) => options.content = Some(value),
                Err(status) => return Ok(Err(status)),
            },
            "--outputs" => match valued("--outputs", run)? {
                Ok(value) => options.outputs = value,
                Err(status) => return Ok(Err(status)),
            },
            "--existing" => match valued("--existing", run)? {
                Ok(value) => options.existing = value,
                Err(status) => return Ok(Err(status)),
            },
            "--ci" => match valued("--ci", run)? {
                Ok(value) => options.ci = value,
                Err(status) => return Ok(Err(status)),
            },
            "--no-detect" => options.no_detect = true,
            "--no-sync" => options.run_sync = false,
            "--no-templates" => options.no_templates = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--dry-run" => options.dry_run = true,
            "--help" | "-h" => {
                run.say(HELP)?;
                return Ok(Err(0));
            }
            flag if flag.starts_with("--tools=") => {
                options.tools = Some(flag["--tools=".len()..].to_string());
            }
            flag if flag.starts_with("--content=") => {
                options.content = Some(flag["--content=".len()..].to_string());
            }
            flag if flag.starts_with("--outputs=") => {
                options.outputs = flag["--outputs=".len()..].to_string();
            }
            flag if flag.starts_with("--existing=") => {
                options.existing = flag["--existing=".len()..].to_string();
            }
            flag if flag.starts_with("--ci=") => {
                options.ci = flag["--ci=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                run.tell(&format!(
                    "{}: Unknown flag: {flag}\nRun {} for usage.\n",
                    style.red("Error"),
                    style.cyan("agentsync init --help")
                ))?;
                return Ok(Err(1));
            }
            value => {
                if options.target.is_some() {
                    run.tell(&format!(
                        "{}: Unexpected argument: {value}\n",
                        style.red("Error")
                    ))?;
                    return Ok(Err(1));
                }
                options.target = Some(value.to_string());
            }
        }
    }
    Ok(Ok(options))
}

/// `_init_normalize_csv`: spaces dropped inside each token, empties skipped,
/// first occurrence kept.
fn normalize_csv(csv: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in csv.split(',') {
        let token: String = token.chars().filter(|c| *c != ' ').collect();
        if token.is_empty() || out.contains(&token) {
            continue;
        }
        out.push(token);
    }
    out
}

/// `_init_merge_lists`.
fn merge_lists(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in a.iter().chain(b) {
        if !token.is_empty() && !out.contains(token) {
            out.push(token.clone());
        }
    }
    out
}

/// `_init_detect_enabled_tools`.
fn detect_tools(root: &str) -> Vec<String> {
    DETECTORS
        .iter()
        .filter(|(_, markers)| {
            markers
                .iter()
                .any(|marker| Path::new(root).join(marker).exists())
        })
        .map(|(tool, _)| tool.to_string())
        .collect()
}

/// The destinations a tool's enabled targets name, resolved inside the project.
fn tool_dests(
    project: &Project,
    paths: &Paths,
    slug: &str,
    log: &mut Log,
) -> Result<Vec<String>, Error> {
    let tool = Tool::load(project, slug)?;
    let mut dests = Vec::new();
    for key in TARGET_KEYS {
        if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
            continue;
        }
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        if let Some(abs) = paths.resolve_dest(&raw, &format!("targets.{key}.dest for {slug}"), log)
        {
            dests.push(abs);
        }
    }
    Ok(dests)
}

/// `_init_existing_dest_files`: repo-relative files under the selected tools'
/// destinations, in byte order, once each.
fn existing_dest_files(
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let paths = Paths::on_disk(target);
    let prefix = format!("{target}/");
    let mut found = BTreeSet::new();
    for slug in tools {
        for abs in tool_dests(project, &paths, slug, &mut Log::default())? {
            let path = Path::new(&abs);
            if path.is_file() {
                found.insert(abs.strip_prefix(&prefix).unwrap_or(&abs).to_string());
            } else if path.is_dir() {
                let mut files = Vec::new();
                files_below(path, &mut files);
                for file in files {
                    let file = file.to_string_lossy().into_owned();
                    found.insert(file.strip_prefix(&prefix).unwrap_or(&file).to_string());
                }
            }
        }
    }
    Ok(found.into_iter().collect())
}

/// `find <dir> -type f`.
fn files_below(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            files_below(&path, found);
        } else if meta.is_file() {
            found.push(path);
        }
    }
}

/// `_init_collect_backup_targets`; a destination that cannot be resolved is
/// reported the way `resolve_dest_path` logs it and skipped.
fn backup_targets(
    run: &mut Run,
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let mut targets = vec![
        format!("{target}/.ai/src"),
        format!("{target}/.ai/agent_sync.yaml"),
        format!("{target}/.ai/.template-manifest"),
    ];
    let paths = Paths::on_disk(target);
    let mut log = Log::default();
    for slug in tools {
        targets.extend(tool_dests(project, &paths, slug, &mut log)?);
    }
    for (_, line) in log.lines() {
        run.tell(&format!("{line}\n"))?;
    }
    Ok(targets)
}

/// `_init_print_plan`.
fn plan(
    style: &Style,
    target: &str,
    tools: &[String],
    content: &[String],
    detect_source: &str,
    no_templates: bool,
) -> String {
    let mut text = format!(
        "{}\n  Target:   {}\n",
        style.bold("Plan:"),
        style.cyan(&format!("{target}/.ai/"))
    );
    if content.is_empty() {
        text.push_str(&format!("  Content:  {}\n", style.dim("(none)")));
    } else if no_templates {
        text.push_str(&format!(
            "  Content:  {} {}\n",
            content.join(", "),
            style.dim("(no starter templates)")
        ));
    } else {
        text.push_str(&format!("  Content:  {}\n", content.join(", ")));
    }
    if tools.is_empty() {
        text.push_str(&format!(
            "  Tools:    {}\n",
            style.dim("(none — opt in later via 'agentsync enable')")
        ));
    } else {
        text.push_str(&format!(
            "  Tools:    {} {}\n",
            tools.join(", "),
            style.dim(&format!("({detect_source})"))
        ));
        let mut any_payload = false;
        for resource in ["settings", "hooks"] {
            let names: Vec<String> = tools
                .iter()
                .flat_map(|slug| catalog::base_payloads(resource, slug))
                .filter_map(|file| Some(file.path().file_name()?.to_string_lossy().into_owned()))
                .collect();
            if !names.is_empty() {
                any_payload = true;
                text.push_str(&format!(
                    "  {:<9} {}\n",
                    format!("{resource}:"),
                    names.join(", ")
                ));
            }
        }
        if !any_payload {
            text.push_str(&format!(
                "  {}\n",
                style.dim("No payloads to scaffold — tools will use base templates at sync time.")
            ));
        }
    }
    text.push('\n');
    text
}

struct Scaffold<'a> {
    target: &'a str,
    ai_dir: &'a str,
    content: &'a [String],
    tools: &'a [String],
    no_templates: bool,
    outputs: &'a str,
    adopt: bool,
    existing: &'a [String],
    ci_github: bool,
    detect_source: &'a str,
    run_sync: bool,
    shown_backup: &'a str,
}

enum Failure {
    Io(Error),
    Signal(i32),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure::Io(e)
    }
}

fn checkpoint(interrupt: &Interrupt) -> Result<(), Failure> {
    match interrupt.received() {
        Some(sig) => Err(Failure::Signal(sig)),
        None => Ok(()),
    }
}

/// The writes between `backup_create` and the summary, each one a point where
/// a failure or a signal restores the snapshot.
fn scaffold(run: &mut Run, interrupt: &mut Interrupt, s: Scaffold) -> Result<(), Failure> {
    let style = run.style;
    let has = |section: &str| s.content.iter().any(|c| c == section);
    let src = format!("{}/src", s.ai_dir);

    create_dir(&src)?;
    if has("rules") {
        create_dir(&format!("{src}/rules"))?;
    }
    if has("skills") {
        create_dir(&format!("{src}/skills"))?;
    }
    if has("commands") {
        create_dir(&format!("{src}/commands"))?;
    }
    if has("subagents") {
        create_dir(&format!("{src}/agents"))?;
    }
    checkpoint(interrupt)?;

    if s.no_templates {
        if has("agents") {
            std::fs::write(format!("{src}/AGENTS.md"), b"")
                .map_err(|e| Error::io(format!("{src}/AGENTS.md"), e))?;
        }
    } else {
        for (rel, bytes) in catalog::template_files() {
            let (dir, _) = rel.rsplit_once('/').unwrap_or(("", rel.as_str()));
            let wanted = match dir {
                "" => has("agents"),
                "rules" => has("rules"),
                "commands" => has("commands"),
                "agents" => has("subagents"),
                _ => has("skills"),
            };
            if wanted {
                write_template(Path::new(&format!("{src}/{rel}")), bytes)?;
            }
        }
    }
    checkpoint(interrupt)?;

    let mut payload_lines = Vec::new();
    for resource in ["settings", "hooks"] {
        for slug in s.tools {
            for file in catalog::base_payloads(resource, slug) {
                let name = file
                    .path()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or(&name);
                let rel = format!("tools/{slug}/{resource}.{ext}");
                write_template(Path::new(&format!("{src}/{rel}")), file.contents())?;
                payload_lines.push(rel);
            }
        }
    }
    checkpoint(interrupt)?;

    let config_file = format!("{}/agent_sync.yaml", s.ai_dir);
    if !Path::new(&config_file).is_file()
        && !Path::new(&format!("{}/agent_sync.yaml", s.target)).is_file()
    {
        std::fs::write(
            &config_file,
            project_config_text(run.env.version, s.tools, s.outputs),
        )
        .map_err(|e| Error::io(&config_file, e))?;
    }
    checkpoint(interrupt)?;

    let mut manifest = TemplateManifest::load(Path::new(s.target))?;
    let templates = catalog::template_files();
    manifest.heal_from_match(
        templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
        Path::new(&src),
    );
    manifest.write(Path::new(s.target))?;
    checkpoint(interrupt)?;

    if !s.existing.is_empty() && s.adopt {
        run.say("\n")?;
        adopt_existing(run, s.target, s.existing)?;
    }
    checkpoint(interrupt)?;

    if s.ci_github {
        write_ci_workflow(run, s.target)?;
    }
    checkpoint(interrupt)?;

    run.say(&summary(
        style,
        s.ai_dir,
        s.tools,
        &payload_lines,
        s.detect_source,
        s.no_templates,
        s.outputs,
        s.run_sync,
    ))?;
    run.say(&format!("Backup: {}\n\n", s.shown_backup))?;
    Ok(())
}

fn create_dir(path: &str) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|e| Error::io(path, e))
}

/// `_init_adopt_existing`.
fn adopt_existing(run: &mut Run, target: &str, existing: &[String]) -> Result<(), Error> {
    let style = run.style;
    let project = Project::at(target)?;
    let sources = adopt::discover_sources(&project)?;
    let mut resolver = Resolver::new(&project, sources)?;
    let mut claimed: Vec<String> = Vec::new();
    let mut skips: Vec<String> = Vec::new();
    let mut adopted = 0;
    for file in existing {
        match resolver.resolve(&format!("{target}/{file}"), run.err)? {
            Ok(found) => {
                if claimed.contains(&found.source_rel) {
                    skips.push(format!(
                        "{file} — another file already became {}",
                        found.source_rel
                    ));
                    continue;
                }
                adopt::copy_into_source(&found)?;
                claimed.push(found.source_rel.clone());
                run.say(&format!(
                    "   {} {} → {}\n",
                    style.green("Adopted"),
                    style.cyan(file),
                    style.dim(&found.source_rel)
                ))?;
                adopted += 1;
            }
            Err(reason) => skips.push(format!("{file} — {reason}")),
        }
    }
    for note in &skips {
        run.say(&format!("   {} {note}\n", style.yellow("Kept as-is")))?;
    }
    if !skips.is_empty() {
        run.say(&format!(
            "   {}\n",
            style.dim("Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.")
        ))?;
    }
    if adopted > 0 || !skips.is_empty() {
        run.say("\n")?;
    }
    Ok(())
}

/// `_init_write_ci_workflow`.
fn write_ci_workflow(run: &mut Run, target: &str) -> Result<(), Error> {
    let style = run.style;
    let dest = format!("{target}/.github/workflows/agentsync-check.yml");
    if Path::new(&dest).is_file() {
        return run.say(&format!(
            "   {} {} {}\n",
            style.yellow("Kept"),
            style.cyan(".github/workflows/agentsync-check.yml"),
            style.dim("(already exists)")
        ));
    }
    create_dir(&format!("{target}/.github/workflows"))?;
    let text = catalog::CI_GITHUB_WORKFLOW
        .replace("__AGENTSYNC_VERSION__", run.env.version)
        .replace(
            "__AGENTSYNC_INSTALL_URL__",
            &format!("https://raw.githubusercontent.com/{REPO}/main/install.sh"),
        );
    staging::write_beside(Path::new(&dest), text.as_bytes())?;
    run.say(&format!(
        "   Created {} — CI gate (agentsync check)\n",
        style.cyan(".github/workflows/agentsync-check.yml")
    ))
}

/// `_init_create_project_config`'s text.
fn project_config_text(version: &str, tools: &[String], outputs: &str) -> String {
    let enabled = if tools.is_empty() {
        "  enabled: []\n".to_string()
    } else {
        let mut text = "  enabled:\n".to_string();
        for tool in tools {
            text.push_str(&format!("    - {tool}\n"));
        }
        text
    };
    format!(
        "# AgentSync — Project Configuration
# All keys are optional — remove any that you leave at the default.

agentsync_version: \"{version}\"
format: {}

# Tools: which ones to sync for this project.
# Each name must match a base tool (see `agentsync list`) or a custom override
# file under .ai/src/tools/<name>.yaml.
tools:
{enabled}
# Source paths (override if you use a custom layout).
source:
  agents: \".ai/src/AGENTS.md\"
  rules: \".ai/src/rules\"
  skills: \".ai/src/skills\"
  commands: \".ai/src/commands\"
  subagents: \".ai/src/agents\"
  tools: \".ai/src/tools\"

# Global defaults applied to all tools.
defaults:
  enabled: false
  cleanup: true

# Post-sync hooks run arbitrary shell — enabling them requires an out-of-repo
# signal (AGENTSYNC_ALLOW_POST_SYNC=true or allow: true in the install-dir
# config.yaml), never this in-repo file. `skip: true` here always disables them.
post_sync:
  skip: false

# Where generated tool files live.
#   committed — outputs and .ai/.sync-manifest are committed; teammates get
#               them from `git pull` and CI runs `agentsync check`.
#   local     — outputs and the manifest are gitignored; every clone runs
#               `agentsync sync` (see `agentsync setup-hooks`).
outputs: {outputs}

# .gitignore management (false leaves the managed block untouched).
gitignore:
  update: true
",
        format_rev::engine()
    )
}

/// `*.md` files directly inside a directory.
fn count_md(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name().to_string_lossy().ends_with(".md")
                        && !e.file_name().to_string_lossy().starts_with('.')
                        && e.path().is_file()
                })
                .count()
        })
        .unwrap_or(0)
}

/// Non-hidden subdirectories of a directory.
fn count_dirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && e.path().is_dir())
                .count()
        })
        .unwrap_or(0)
}

/// `_init_print_summary`.
#[allow(clippy::too_many_arguments)]
fn summary(
    style: &Style,
    ai_dir: &str,
    tools: &[String],
    payload_lines: &[String],
    detect_source: &str,
    no_templates: bool,
    outputs: &str,
    run_sync: bool,
) -> String {
    let src = Path::new(ai_dir).join("src");
    let mut text = String::from("\n");
    if outputs == "committed" {
        text.push_str(&format!(
            "   Created {}     — project config (outputs: committed — teammates need only git pull)\n",
            style.cyan(".ai/agent_sync.yaml")
        ));
    } else {
        text.push_str(&format!(
            "   Created {}     — project config (outputs: local — every clone runs agentsync sync)\n",
            style.cyan(".ai/agent_sync.yaml")
        ));
    }
    let agents = src.join("AGENTS.md");
    if agents.is_file() {
        let empty = std::fs::metadata(&agents)
            .map(|m| m.len() == 0)
            .unwrap_or(false);
        if no_templates && empty {
            text.push_str(&format!(
                "   Created {}      — {}\n",
                style.cyan(".ai/src/AGENTS.md"),
                style.dim("(empty)")
            ));
        } else {
            text.push_str(&format!(
                "   Created {}      — agent identity\n",
                style.cyan(".ai/src/AGENTS.md")
            ));
        }
    }
    let sections: [(&str, &str, &str, bool); 4] = [
        ("rules", ".ai/src/rules/", "rule(s)", false),
        ("skills", ".ai/src/skills/", "skill(s)", true),
        ("commands", ".ai/src/commands/", "command(s)", false),
        ("agents", ".ai/src/agents/", "subagent(s)", false),
    ];
    for (dir, shown, noun, dirs) in sections {
        let path = src.join(dir);
        if !path.is_dir() {
            continue;
        }
        let count = if dirs {
            count_dirs(&path)
        } else {
            count_md(&path)
        };
        let padded = pad_created(style, shown);
        if count > 0 {
            text.push_str(&format!("{padded}— {count} {noun}\n"));
        } else if no_templates {
            text.push_str(&format!("{padded}— {}\n", style.dim("(empty)")));
        }
    }
    for line in payload_lines {
        text.push_str(&format!(
            "   Created {}\n",
            style.cyan(&format!(".ai/src/{line}"))
        ));
    }
    text.push('\n');
    if tools.is_empty() {
        text.push_str(&format!(
            "   {}\n",
            style.dim("No tools enabled. Run 'agentsync enable <slug>' to opt in.")
        ));
    } else {
        let joined = tools.join(", ");
        let count = tools.len();
        let line = match detect_source {
            "detect" => format!(
                "   {} {joined}\n",
                style.green(&format!("Auto-detected {count} tool(s):"))
            ),
            "flag" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(from --tools)")
            ),
            "mixed" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(auto-detect + --tools)")
            ),
            "interactive" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(selected)")
            ),
            _ => format!(
                "   {} {joined}\n",
                style.green(&format!("Enabled {count} tool(s):"))
            ),
        };
        text.push_str(&line);
    }
    text.push_str(&format!("\n{}\n\nNext steps:\n", style.green("Done!")));
    let mut step = 1;
    if agents.is_file() {
        text.push_str(&format!(
            "  {step}. Edit {} — customize your agent's identity\n",
            style.cyan(".ai/src/AGENTS.md")
        ));
        step += 1;
    }
    text.push_str(&format!(
        "  {step}. Run {}    — print an AI prompt to tailor .ai/src/ to your codebase\n",
        style.cyan("agentsync generate")
    ));
    step += 1;
    text.push_str(&format!(
        "  {step}. Run {}        — browse all available tools\n",
        style.cyan("agentsync list")
    ));
    step += 1;
    if tools.is_empty() {
        text.push_str(&format!(
            "  {step}. Run {} — opt in to tools you use\n",
            style.cyan("agentsync enable <slug>")
        ));
    } else {
        text.push_str(&format!(
            "  {step}. Run {} — add more tools\n",
            style.cyan("agentsync enable <slug>")
        ));
    }
    step += 1;
    if run_sync && !tools.is_empty() {
        text.push_str(&format!(
            "  {step}. Re-run {}     — after every change to .ai/src/\n",
            style.cyan("agentsync sync")
        ));
    } else {
        text.push_str(&format!(
            "  {step}. Run {}        — distribute to enabled tools\n",
            style.cyan("agentsync sync")
        ));
    }
    text.push_str(&format!(
        "\nCustomize:\n  {} {}            — configure shared MCP servers\n  {} {} — override settings/hooks per tool\n\n",
        style.dim("•"),
        style.cyan("agentsync add mcp <server>"),
        style.dim("•"),
        style.cyan("agentsync customize <tool> <resource>")
    ));
    text
}

/// `   Created $(_cyan "<shown>")` padded as Bash's literal spacing pads each
/// section line: the styled name plus the spaces that bring the plain text to
/// the column the `—` starts in.
fn pad_created(style: &Style, shown: &str) -> String {
    let spaces = match shown {
        ".ai/src/rules/" => 10,
        ".ai/src/skills/" => 9,
        ".ai/src/commands/" => 7,
        _ => 9,
    };
    format!("   Created {}{}", style.cyan(shown), " ".repeat(spaces))
}

fn prune(run: &mut Run, target: &str, retention: backup::Retention) -> Result<(), Error> {
    let (limit, max_age) = (run.env.backup_limit.clone(), run.env.backup_max_age.clone());
    if let Err(e) = backup::prune(target, limit.as_deref(), max_age.as_deref(), retention) {
        report_backup_error(run, &e)?;
        let style = run.style;
        run.tell(&format!(
            "{}: Could not prune old AgentSync backups.\n",
            style.yellow("Warning")
        ))?;
    }
    Ok(())
}

/// `_backup_error`: `Error: <message>` on stderr, plain.
fn report_backup_error(run: &mut Run, error: &Error) -> Result<(), Error> {
    match error {
        Error::Backup(message) => run.tell(&format!("Error: {message}\n")),
        other => run.tell(&format!("{other}\n")),
    }
}

```

In `src/main.rs`, before the `refresh` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("init") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
        let style = Style::for_stdout();
        let sync_env = sync_env();
        let colors = log_colors();
        let mut sync = |root: &str| cli::sync::run(root, &[], &sync_env, colors, streams());
        let mut confirm =
            |question: &str, default_yes: bool| prompts::confirm(question, default_yes);
        let mut multiselect = |title: &str, options: &[String], preselected: &[String]| {
            prompts::multiselect_on_terminal(title, options, preselected, &style)
        };
        let mut env = cli::init::Env {
            version: engine_version(),
            cwd,
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
            backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            interactive: prompts::is_tty(),
            confirm: &mut confirm,
            multiselect: &mut multiselect,
            sync: &mut sync,
        };
        return cli::init::init(
            &rest,
            &style,
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `init`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init "
```

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in init init_flow adopt outputs_mode version_pin hooks; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'parity: init' tests/native_parity.bats
```

Expected: `255 passed`, `0`, `11`, `1`; `0` for every file (`init` outside the sandbox for its wizard case); `ok 1` and `ok 2`.

- [ ] **Step 5: Prove the fixture bites, check the wizard on a terminal, lint, commit**

Change `Initializing AgentSync` to `Initialising AgentSync` in `src/cli/init.rs`, rebuild, rerun `bats --tap -f 'init scaffolds' tests/native_parity.bats`: `not ok 1` with the `Initializing AgentSync in <root>` line in the diff; revert and rebuild.

Recreate the three harnesses when the session scratchpad no longer holds `phase4i/`. The reference and the pty harness take `<engine 0|1> <repo root> <out file>` and mask the project, the templates path, the overlay directories, and the backup ids; the suite runner takes `<repo root> <0|1|both> <out file>`. The pty harness needs `script`, which the agent sandbox refuses, so it runs outside the sandbox, and types each key after a pause; all three use macOS `stat -f%Lp` for modes.

`phase4i/init_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: init_reference.sh <engine 0|1> <repo root> <out file>
# Runs every non-interactive init branch in fresh git projects and prints
# status, masked output, the resulting tree with hashes and modes, and each
# backup's targets and files.
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
unset AGENTSYNC_REPO_ROOT AGENTSYNC_BACKUP_LIMIT AGENTSYNC_BACKUP_MAX_AGE_DAYS AGENTSYNC_CONFIG_PATH
N=0
P=""
fresh() {
    N=$((N + 1))
    P="$WORK/p$N"
    mkdir -p "$P"
    (cd "$P" && git init --quiet && git config user.email t@t && git config user.name T)
    cd "$P" || exit 1
}
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    local from
    for from in "$REPO/lib/templates" "~/${REPO#"$HOME"/}/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/<engine>/lib/templates}
    done
    printf '%s\n' "$text" | sed -E \
        -e 's#[^ ]*/agentsync_shared\.[A-Za-z0-9]+/src#<overlay>/src#g' \
        -e 's#/<agentsync-overlay>/[a-z-]+/src#<overlay>/src#g' \
        -e 's#[0-9]{8}T[0-9]{6}Z-(sync|rollback|init)-[0-9]+(-[0-9]+)?#<backup-id>#g'
}
report() {
    local name="$1" rc="$2" output="$3"
    {
        echo "### $name"
        echo "rc=$rc"
        printf '%s' "$output" | mask
        echo "--- tree"
        (cd "$P" && find . -type f ! -path './.git/*' ! -path '*/.ai/backups/*' -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' f; do
            printf '%s %s %s\n' "$(shasum -a 256 "$f" | cut -c1-12)" "$(stat -f%Lp "$f")" "$f"
        done)
        echo "--- backups"
        (cd "$P" && for b in .ai/backups/*/; do
            [[ -d "$b" ]] || continue
            printf 'snapshot %s\n' "$(grep '^operation=' "$b/metadata" 2>/dev/null)"
            LC_ALL=C sort "$b/targets.tsv" 2>/dev/null | sed 's/^/  /'
            (cd "$b" && find files -type f 2>/dev/null | LC_ALL=C sort | sed 's/^/  /')
            [[ -f "$b/after.tsv" ]] && echo "  sealed"
        done; [[ -f .ai/backups/.latest ]] && echo "latest: $(cat .ai/backups/.latest | mask)")
        echo
    } >> "$OUT"
}
run() {
    local name="$1"; shift
    local rc=0 output
    output=$(cd "$P" && bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    report "$name :: agentsync $*" "$rc" "$output"
}
run_in() {
    local name="$1" dir="$2"; shift 2
    local rc=0 output
    output=$(cd "$P/$dir" && bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    report "$name :: (in $dir) agentsync $*" "$rc" "$output"
}

fresh; run "help" init --help
run "unknown-flag" init --bogus
run "unexpected-arg" init a b
run "tools-missing-value" init --tools
run "outputs-bogus" init --outputs bogus
run "existing-bogus" init --existing bogus
run "ci-bogus" init --ci gitlab
run "dir-missing" init missing-dir
run "content-bogus" init --content bogus
run "content-bogus-with-tools" init --tools claude --content agents,bogus
mkdir -p .ai; run_in "inside-ai" .ai init
run "plain" init
run "again-skips" init
run "again-skips-with-flags" init --tools claude --no-sync

fresh; run "no-detect" init --no-detect
fresh; run "yes-no-detect" init --yes --no-detect
fresh; mkdir -p .claude .cursor; run "detect-two-no-sync" init --no-sync
fresh; mkdir -p .claude .cursor; run "detect-two-sync" init
fresh; mkdir -p .claude .cursor .github/instructions .gemini .codex .kimi-code .opencode .windsurf .junie .amazonq .zed .agents/rules; touch .clinerules
run "detect-all-dry" init --dry-run
run "detect-all-no-sync" init --no-sync
fresh; echo "# generic" > AGENTS.md; touch CLAUDE.md GEMINI.md .cursorrules .windsurfrules opencode.json .rules; run "detect-files-dry" init --dry-run
fresh; mkdir -p .cursor; run "tools-union-detect-no-sync" init --tools claude --no-sync
fresh; run "tools-claude-sync" init --tools claude
fresh; run "tools-claude-local-sync" init --tools claude --outputs local
fresh; run "tools-claude-no-sync" init --tools claude --no-sync
fresh; run "tools-two-content-two" init --tools claude,cursor --content agents,rules --no-sync
fresh; run "eq-forms" "init" "--tools=claude" "--content=rules" "--outputs=local" "--existing=adopt" "--no-sync"
fresh; run "spaced-dupes" init --tools "claude, cursor,claude" --content " rules , skills " --no-detect --no-sync
fresh; run "no-templates" init --no-templates --no-detect
fresh; run "no-templates-agents-rules" init --no-templates --no-detect --content agents,rules
fresh; run "no-templates-rules-only" init --no-templates --no-detect --content rules
fresh; run "dry-two-tools" init --dry-run --tools claude,cursor
fresh; run "dry-content" init --dry-run --content agents,rules
fresh; run "dry-no-templates" init --dry-run --no-templates --no-detect
fresh; run "dry-no-detect" init --dry-run --no-detect
fresh; run "dry-tools-outputs-local" init --dry-run --tools claude --outputs local --ci github

fresh; printf '# Hand-written team rules\n' > CLAUDE.md; mkdir -p .claude/rules; printf '# Legacy rule\n' > .claude/rules/legacy.md; printf '{"settings": true}\n' > .claude/settings.json
run "adopt-existing-sync" init --tools claude --yes
fresh; printf '# Hand-written team rules\n' > CLAUDE.md; mkdir -p .claude/rules; printf '# Legacy rule\n' > .claude/rules/legacy.md
run "adopt-existing-no-sync" init --tools claude --yes --no-sync
fresh; printf '# Hand-written team rules\n' > CLAUDE.md; mkdir -p .claude/rules; printf '# Legacy rule\n' > .claude/rules/legacy.md
run "replace-existing" init --tools claude --yes --existing replace --no-sync
fresh; printf '# From CLAUDE\n' > CLAUDE.md; printf '# From AGENTS\n' > AGENTS.md
run "two-dests-one-source" init --tools claude,codex --yes --no-sync
fresh; mkdir -p .cursor/rules; printf 'body\n' > .cursor/rules/core.mdc; printf '{}\n' > .cursor/mcp.json
run "refused-transformed" init --tools cursor --yes --no-sync
fresh; mkdir -p .claude; printf 'claude-before\n' > CLAUDE.md; printf 'settings-before\n' > .claude/settings.json
run "backup-existing-dests" init --tools claude --no-sync

fresh; mkdir -p .github; run "ci-github" init --tools claude --yes --ci github --no-sync
fresh; run "ci-github-no-github-dir" init --tools claude --yes --ci github --no-sync
fresh; mkdir -p .github/workflows; printf 'name: mine\n' > .github/workflows/agentsync-check.yml
run "ci-existing-kept" init --tools claude --yes --ci github --no-sync
fresh; run "ci-local" init --tools claude --yes --ci github --outputs local --no-sync

fresh; mkdir -p subdir; run "subdir" init subdir --no-detect
fresh; mkdir -p .ai; printf 'tools:\n  enabled:\n    - claude\n' > .ai/agent_sync.yaml
run "preexisting-config" init --no-detect --no-sync
fresh; printf 'tools:\n  enabled: []\n' > agent_sync.yaml
run "preexisting-root-config" init --no-detect --no-sync
fresh; mkdir -p .ai; printf 'backup:\n  retention: typo\n' > .ai/agent_sync.yaml
run "retention-typo" init --no-detect
fresh; mkdir -p .ai/agent_sync.yaml; printf 'keep\n' > .ai/agent_sync.yaml/sentinel
run "scaffold-failure-restores" init --no-detect
run_env() {
    local name="$1" assignment="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && env "$assignment" bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    report "$name :: $assignment agentsync $*" "$rc" "$output"
}
fresh; run_env "bad-backup-limit" AGENTSYNC_BACKUP_LIMIT=abc init --no-detect
fresh; run_env "preserve-retention-env-ignored" AGENTSYNC_BACKUP_MAX_AGE_DAYS=0 init --no-detect --no-sync
```

`phase4i/init_tty.sh`:

```bash
#!/usr/bin/env bash
# Usage: init_tty.sh <engine 0|1> <repo root> <out file>
# Drives the init wizard on a pseudo-terminal through `script`, so is_tty
# holds and the multiselect reads its keys from the pty. Keys are fed one
# token at a time with a pause between them: a byte written before the
# program switches the terminal to raw mode would sit in the canonical line
# buffer, so the harness types the way a person does. Records the masked
# transcript plus the resulting tree.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/tty_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
export AGENTSYNC_NO_UPDATE_CHECK=1
unset AGENTSYNC_REPO_ROOT
N=0
P=""
fresh() {
    N=$((N + 1))
    P="$WORK/p$N"
    mkdir -p "$P"
    (cd "$P" && git init --quiet)
    cd "$P" || exit 1
}
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    local from
    for from in "$REPO/lib/templates" "~/${REPO#"$HOME"/}/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/<engine>/lib/templates}
    done
    printf '%s\n' "$text" | tr -d '\r' | sed -E \
        -e 's#[^ ]*/agentsync_shared\.[A-Za-z0-9]+/src#<overlay>/src#g' \
        -e 's#/<agentsync-overlay>/[a-z-]+/src#<overlay>/src#g' \
        -e 's#[0-9]{8}T[0-9]{6}Z-(sync|rollback|init)-[0-9]+(-[0-9]+)?#<backup-id>#g'
}
# Types each `|`-separated token after a pause; `\e[B` and the like stay one token.
feed() {
    local keys="$1" token
    sleep 1.5
    while IFS= read -r -d '|' token || [[ -n "$token" ]]; do
        printf '%b' "$token"
        sleep 0.4
    done <<< "$keys|"
    sleep 2
}
# drive <name> <tokens separated by |> <args...>
drive() {
    local name="$1" keys="$2"; shift 2
    local rc=0 transcript
    # Echo off in the child, so the transcript holds the program's bytes and
    # not the pty's copy of the typed keys; rc is script's, not the feeder's.
    transcript=$(cd "$P" && { feed "$keys" | script -q /dev/null bash -c 'stty -echo; exec bash "$0" "$@"' "$REPO/bin/agentsync.sh" "$@" 2>&1; echo "rc=${PIPESTATUS[1]}"; })
    {
        echo "### $name :: agentsync $*"
        printf '%s' "$transcript" | mask
        echo "--- tree"
        (cd "$P" && find . -type f ! -path './.git/*' ! -path '*/.ai/backups/*' -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' f; do
            printf '%s %s %s\n' "$(shasum -a 256 "$f" | cut -c1-12)" "$(stat -f%Lp "$f")" "$f"
        done)
        echo
    } >> "$OUT"
}

# Enter through both lists, keep committed outputs, proceed.
fresh; drive "defaults" '\n|\n|y\n|y\n' init --no-sync
# Toggle the first two tools, move down four, toggle a third; drop skills; local outputs; proceed.
fresh; drive "pick-tools-and-content" ' |\e[B| |\e[B|\e[B|\e[B|\e[B| |\n|\e[B|\e[B| |\n|n\n|y\n' init --no-sync
# Select all tools, then none, then only the first; keep content; dry run ends before Proceed.
fresh; drive "all-none-first" 'a|n| |\n|\n|y\n' init --no-sync --dry-run
# Cancel the tools list with q.
fresh; drive "cancel-tools-q" 'q' init
# Cancel the content list with Escape, the last byte.
fresh; drive "cancel-content-esc" '\n|\e' init
# Decline at Proceed.
fresh; drive "decline-proceed" '\n|\n|y\n|n\n' init
# Existing config offered for adoption, then a GitHub gate offered; both accepted.
fresh; mkdir -p .github .claude/rules; printf '# Hand-written\n' > CLAUDE.md; printf '# Legacy\n' > .claude/rules/legacy.md
drive "existing-and-ci" '\n|\n|y\n|y\n|y\n|y\n' init --no-sync
# Existing config declined; no .github, so no gate; dry run through the wizard.
fresh; printf '# Hand-written\n' > CLAUDE.md
drive "existing-declined-dry" '\n|\n|y\n|n\n' init --dry-run
# Unknown and vim keys: x is ignored, j and k move, space toggles.
fresh; drive "vim-keys" 'x|j|j|k| |\n|\n|y\n|y\n' init --no-sync --dry-run
```

`phase4i/native_suite.sh`:

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
bash phase4i/init_reference.sh 0 "$PWD" phase4i/ref_bash.out && bash phase4i/init_reference.sh 1 "$PWD" phase4i/ref_native.out
wc -l < phase4i/ref_native.out
diff phase4i/ref_bash.out phase4i/ref_native.out | grep -c '^[<>]'
diff phase4i/ref_bash.out phase4i/ref_native.out | grep '^[<>]' | grep -vc 'Is a directory'
bash phase4i/init_tty.sh 0 "$PWD" phase4i/tty_bash.out && bash phase4i/init_tty.sh 1 "$PWD" phase4i/tty_native.out
wc -l < phase4i/tty_native.out
diff phase4i/tty_bash.out phase4i/tty_native.out | grep -c '^[<>]'
grep -c '^rc=0' phase4i/tty_native.out; grep -c '^rc=130' phase4i/tty_native.out
```

Expected: `2862` lines; `2` differing lines, both the scaffold failure's message (`init.sh: line 415: <root>/.ai/agent_sync.yaml: Is a directory` against `<root>/.ai/agent_sync.yaml: Is a directory (os error 21)`, decision 3), `0` outside it; `753` transcript lines; `0` differences, with the frames, arrows, `a`, `n`, `q`, Escape, the existing-config and CI questions, and colours identical; `6` scenarios at `rc=0` and `3` at `rc=130`.

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/init.rs src/catalog.rs src/cli/adopt.rs src/cli/refresh.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "feat(native): port init"
```

---

### Task 7: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
41. A detected tool whose destination has a file where a directory is expected,
    such as a legacy single-file `.clinerules`, makes `init` refuse at the
    backup step with `Backup target parent is not a directory`.
42. `init` drops every space inside a `--tools` or `--content` token, so
    `cla ude` reads as `claude`; `--tools=` and `--content ''` skip the wizard
    while contributing nothing.
43. `init` heals `.ai/.template-manifest` before it adopts existing outputs, so
    an adopted `AGENTS.md` carries the template's hash and `refresh` treats it
    as a silently kept edit.
```

Append to "Accepted deviations":

```markdown
- Phase 4i: the multiselect switches the terminal through `stty` (settings
  saved with `stty -g`, restored on exit) where Bash's `read -rsn1` did it
  in-process; a lone Escape registers after the same one-second wait.
- Phase 4i: a failed scaffold write in `init` reports the Rust I/O error where
  Bash printed the shell's redirect message (`init.sh: line N: <path>: Is a
  directory`); the restore and the status are unchanged.
- Phase 4i: `init`'s first sync runs in-process where Bash spawned
  `lib/sync.sh`; the transcript is the same.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/init.sh` row to `→ src/cli/{init,upgrade_config}.rs   Phase 4d (upgrade_config) and 4i (init), ported`, the `lib/helpers/prompts.sh` row to `→ src/prompts.rs          confirm on /dev/tty (Phase 3); multiselect through stty (4i)`, and the `lib/helpers/adopt.sh` row to `→ src/cli/adopt.rs        Phase 4f, ported; Resolver serves init's adoption (4i)`. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase4i/native_suite.sh "$PWD" both phase4i/suite_both.out && tail -1 phase4i/suite_both.out
```

Expected: `255 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 49 bats files, each run one at a time under both engines.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-4i-init.md
git commit -m "docs(native): map the phase 4i modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the two parity fixtures pass, and a `## Completion receipt` records the fresh verification. With it the `template_manifest` family is complete; the next plans cover the standalone commands the spec names: `doctor`, `add`, `export`, `import`, `generate`, `shell-init`, and `setup-hooks`.

## Run log

### 2026-09-16 — Phase 4i planned
- Commits: this plan.
- Verified: every non-interactive branch of `cmd_init` was captured with `init_reference.sh` (53 scenarios) and the wizard with `init_tty.sh` (9 scenarios on a pty). The reference turned up four Bash bugs, fixed on a copy of the engine and each covered by a regression test that failed on the committed engine: the unbound `$1` on a missing directory, the last-wins adoption behind a first-wins message (with the lost refusal reason), the wizard's lists never drawn because `is_tty` tested a captured stdout, and `read -t 0.01` rejected by Bash 3.2 so arrows cancelled. The Rust in Tasks 5–6 was drafted in the tree against the fixed Bash: `cargo test` 255/0/11/1, fmt and clippy clean; with `init` in `_NATIVE_COMMANDS`, every bats file ran green under `AGENTSYNC_NATIVE=1` (49 files, 0 failures), the two parity fixtures passed and failed on an `Initialising AgentSync` mutation; `init_reference.sh` gave 2862-line transcripts identical apart from the scaffold failure's I/O message; `init_tty.sh` gave identical 753-line transcripts once the harness typed keys one at a time (a byte written before `read -rsn1` sits in the canonical buffer) and turned echo off in the child. Baseline `cargo test` 243/0/11/1; `init.bats` 35, `init_flow.bats` 14, `adopt.bats` 29, `native_parity.bats` 54 cases green in Bash.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
