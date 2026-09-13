# Rust Migration Phase 2: Render Core and Native `check`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Serve `agentsync check` from the Rust binary: render what `lib/sync.sh --force` would write, in memory, and compare it with the project's managed outputs, with the messages and exit codes of `lib/check.sh`, and no `tar`, temp workspace, or second process.

**Architecture:** The render works on a `Workspace`, an in-memory tree that stands in for the temporary copy `lib/check.sh` made: `.ai/` without backups, a root `agent_sync.yaml`, and the manifest's outputs are indexed from disk, the engine's templates are mounted under the virtual root `/<agentsync>`, and overlays live under `/<agentsync-overlay>`. Every Bash helper `sync` calls for a forced, non-dry run is ported line for line on top of it (`paths`, `filters`, `text`, `convert`, `opencode_json`, `file_ops`, `rules`, `overlay`, `profiles`, tool and payload resolution), and `render` replays `sync.sh`'s personal and profile passes. The test seam stays the one Phase 1 chose, the CLI process boundary: `tests/native_parity.bats` runs the Bash `sync` to produce outputs and asserts both engines' `check` agree byte for byte, which is the golden-output comparison the spec asks for; `cargo test` covers each module with values read off the Bash helpers.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2; dev: assert_cmd 2, predicates 3, tempfile 3 — no new dependency. Bash 3.2 for the dispatcher and the two Bash fixes, bats-core for conformance. Design: `docs/specs/2026-09-12-rust-migration-design.md`, section "Phase 2 — Render core and `check`". Previous phase: `docs/plans/2026-09-12-rust-migration-phase-1-native-list.md` (closed, receipt 2026-09-13).

## Global Constraints

- `.ai/src/` remains the source of truth; the native `check` never writes under a user project, and `src/` has no `std::fs::write` outside `#[cfg(test)]`.
- No binary ships to users in this phase: without a built binary every command runs in Bash exactly as today.
- `bin/agentsync.sh` and `lib/**/*.sh` stay Bash 3.2-compatible and clean under `shellcheck -x -S warning -e SC1091`.
- A ported command matches Bash byte for byte on stdout, stderr, and exit status when stdout is not a terminal; `check` prints no colour in either engine.
- Rust: `unsafe_code = "forbid"`; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean after every task; no YAML or JSON crate — `yaml_subset` mirrors `lib/helpers/yaml.sh`, `opencode_json` mirrors the awk program in `lib/helpers/opencode.sh`.
- Unit tests that touch the disk are `#[cfg(unix)]`; everything else runs on an in-memory `Workspace` rooted at `/proj`, so `cargo test` stays green on the Windows runner.
- `VERSION` is the only version source; `Cargo.toml` keeps `0.0.0` until Phase 5.
- Accepted deviations already in the spec (Phase 1) still hold: clap's exit 2 on stray arguments, byte-order tool slugs, `\r\n` read as `\n`.
- Accepted deviations this plan proposes, recorded in the spec in Task 9's commit and ratified by the plan's review: byte-order directory listings and globs; the log tail of a failed render names project and virtual paths instead of random temp dirs; `Incomplete copy — missing: .ai` instead of `tar`'s diagnostic; `agent_sync.yaml` read with its `shared:` block in place; symlinks under `.ai/` and among outputs followed; non-UTF-8 config and OpenCode JSON read with replacement characters; `printf '%b'` as Bash 3.2 expands it.
- Known quirks reproduced, appended to the spec in the task that ports them: 9 (last `read_frontmatter_field` occurrence wins), 10 (`_rule_paths_csv` collects every frontmatter list item), 11 (`description: >-` indexes a skill as `-`).
- Commits follow Conventional Commits, scope `native` for engine work, imperative subject, no attribution trailers.

## Module closure

`check` runs `lib/check.sh`, which re-runs `lib/sync.sh --force` in a copy. The Bash the tasks read, with the lines that matter:

```text
lib/check.sh                      1-195    pin gate, copy list, shared merge, compare, report
lib/sync.sh                       155-200  config path and source overrides
                                  309-736  dests, the six sync steps, sync_tool, cleanup_tool
                                  740-876  run config, version pin, source detection
                                  911-939  protected dests
                                  997-1233 banner, catalog, personal and profile passes
lib/helpers/paths.sh              8-307    normalise, canonicalise, dest and source resolution
lib/helpers/filters.sh            9-47     matches_filter
lib/helpers/logging.sh            18-101   log voice, display_path
lib/helpers/file_ops.sh           17-199   cleanup_path, copy_file, sync_dir
lib/helpers/manifest.sh           143-188  record_write, was_touched, record_tree
lib/helpers/rule_operations.sh    8-527    headers, merge, sync_rules, imports, command index, command skills
lib/helpers/format_conversion.sh  8-515    frontmatter, converters, _sweep_generated
lib/helpers/opencode.sh           5-513    JSON composition
lib/helpers/shared.sh             56-406   overlays, shared_parent_src
lib/helpers/profiles.sh           35-141   profile names, overlay dir, tools, active
lib/helpers/tool_resolver.sh      67-429   layered values, flags, filters, payload resolution
lib/helpers/version.sh            14-28    pin and hint
```

Out of this phase, and left to Phase 3 with `sync` itself: manifest load, drift, and write; backups; `.gitignore`; `--dry-run`, `--only`, `--skip`, `--profile`, `--if-stale`, `--workspace`; post-sync hook execution; the `shared:` overlay as `sync` builds it (`check` merges the parent before rendering, as `lib/check.sh` does); log lines of the transaction (drift, baseline, summary).

---

### Task 0: Baseline

**Files:**
- None changed.

**Interfaces:**
- Consumes: branch `feat/native-engine-phase-1` with Phase 1 closed; `cargo` on `PATH` (or `~/.cargo/bin/cargo`).
- Produces: a recorded green baseline to count against.

- [x] **Step 1: Confirm the branch and the toolchain**

```bash
git branch --show-current
git status --short
cargo --version
```

Expected: `feat/native-engine-phase-1`; empty status; `cargo 1.85` or newer.

- [x] **Step 2: Record the baseline**

```bash
cargo test 2>&1 | grep 'test result'
bats --jobs 4 tests/ --tap | head -1
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
```

Expected: `35 passed` (unit) and `7 passed` (integration); the TAP plan is `1..745` with no `not ok`; ShellCheck exits 0.

---

### Task 0b: Prefactor — A Missing Source Stays in the Project

**Files:**
- Modify: `lib/helpers/paths.sh:252-302` (`resolve_source_path_r`)
- Modify: `tests/paths.bats` (one regression test)
- Modify: `tests/shared.bats:43-71` (`_shared_make_sparse_pair`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `resolve_source_path_r` returns the project path for a source that does not exist instead of falling back to `$DEFAULT_REPO_ROOT/<path>`. Found while mapping the source resolution: a project with only `.ai/src/AGENTS.md` syncs the engine checkout's own `.ai/src/rules` — seven maintainer rules, `native-engine.md` among them — into `.claude/rules`, because the fallback resolves `.ai/src/rules` against the install directory, which `install.sh` fills with a `git clone`. A binary has no checkout to fall back to, so the parity reference must not either. The fix changes one documented-by-test behaviour: `tests/shared.bats` built a child project without any `AGENTS.md`, and its sync only passed because it read the engine's. The recommended resolution keeps `AGENTS.md` required (`Source agents file not found`, the existing error) and gives the fixture an `AGENTS.md` outside `.ai/src/`, which keeps that test's intent — an overlay without `AGENTS.md`. **This is a decision for the plan's review**: the alternative, a sync that tolerates a project without `AGENTS.md`, is a product change and would need its own task.

- [x] **Step 1: Write the failing test at the end of `tests/paths.bats`**

```bash
@test "paths: resolve_source_path keeps a missing project source in the project" {
    mkdir -p "$DEFAULT_REPO_ROOT/.ai/src/rules"
    resolve_source_path_r ".ai/src/rules" "source.rules"
    [ "$REPLY" = "$REPO_ROOT/.ai/src/rules" ]
}
```

- [x] **Step 2: Run it, confirm it fails**

Run: `bats tests/paths.bats`
Expected: the new test fails at `[ "$REPLY" = "$REPO_ROOT/.ai/src/rules" ]` (the reply is `…/engine/.ai/src/rules`); the other 26 pass.

- [x] **Step 3: Drop the engine fallback from `resolve_source_path_r` in `lib/helpers/paths.sh`**

Replace the whole function with:

```bash
resolve_source_path_r() {
    local raw_path="$1"
    local label="$2"

    if [[ -z "$raw_path" ]]; then
        log_error "$label is empty"
        return 1
    fi

    normalize_absolute_path_r "$raw_path"
    local abs_path_target="$REPLY"
    local canonical_path_target=""
    if canonicalize_with_existing_ancestor_r "$abs_path_target" 2>/dev/null; then
        canonical_path_target="$REPLY"
    fi

    if [[ -n "$canonical_path_target" ]] && ! is_path_safe_source "$canonical_path_target"; then
        log_error "$label resolves outside safe source roots: $raw_path -> $canonical_path_target"
        return 1
    fi

    REPLY="$abs_path_target"
    return 0
}
```

- [x] **Step 4: Run the suite, confirm the one fixture that relied on the fallback**

Run: `bats --jobs 4 tests/ --tap | grep '^not ok'`
Expected: exactly one line, `not ok … shared: sync succeeds when overlay omits commands, agents, and AGENTS.md` — its child has no `AGENTS.md`, and sync now says `Source agents file not found`.

- [x] **Step 5: Give the sparse fixture an `AGENTS.md` outside `.ai/src/`**

In `tests/shared.bats`, `_shared_make_sparse_pair`, after the line `echo "child-rule" > "$child_dir/.ai/src/rules/child-only.md"`, add:

```bash
    echo "# Child" > "$child_dir/.ai/AGENTS.md"
    sed 's|agents: ".ai/src/AGENTS.md"|agents: ".ai/AGENTS.md"|' "$child_dir/.ai/agent_sync.yaml" > "$child_dir/.ai/agent_sync.yaml.tmp"
    mv "$child_dir/.ai/agent_sync.yaml.tmp" "$child_dir/.ai/agent_sync.yaml"
```

`init` writes `source.agents: ".ai/src/AGENTS.md"` explicitly, so the flat-layout detection alone does not find `.ai/AGENTS.md`; the config line has to move with the file.

- [x] **Step 6: Run the affected files, confirm green**

```bash
bats tests/paths.bats tests/shared.bats tests/sync_options.bats tests/resource_resolver.bats
shellcheck -x -S warning -e SC1091 lib/helpers/paths.sh
bats --jobs 4 tests/ --tap | grep -c '^not ok'
```

Expected: 27 + 12 + 17 + 19 tests pass; ShellCheck exits 0; `0`. The full plan is now `1..746`.

- [x] **Step 7: Commit**

```bash
git add lib/helpers/paths.sh tests/paths.bats tests/shared.bats
git commit -m "fix(sync): resolve a missing source inside the project, not the engine"
```

---

### Task 0c: Prefactor — `check` Inherits Only What `sync` Inherits

**Files:**
- Modify: `lib/helpers/shared.sh` (new `shared_inherit_categories` after `shared_parent_src`)
- Modify: `lib/check.sh:140`
- Modify: `tests/check.bats` (one regression test)

**Interfaces:**
- Consumes: nothing new.
- Produces: `shared_inherit_categories "<raw>"` prints the CSV of `rules|skills|commands|agents` tokens (`subagents` read as `agents`), and `lib/check.sh` builds its overlay from it. Found while mapping `check`: `sync`'s `shared_setup_overlay` skips an unknown category with a warning, but `check.sh` passed the raw `shared.inherit` to `build_overlay_tree`, so `inherit: rules, tools` copied the parent's tool YAMLs into `check`'s workspace and `check` reported drift right after a clean `sync` (`Missing: OTHER.md` for a parent `claude.yaml` with another agents dest). The native `check` needs a Bash reference that agrees with `sync`.

- [x] **Step 1: Write the failing test at the end of `tests/check.bats`**

```bash
@test "check agrees with sync when shared.inherit names a category sync skips" {
    mkdir -p parent/.ai/src/rules parent/.ai/src/tools
    printf 'parent rule\n' > parent/.ai/src/rules/parent-only.md
    printf 'targets:\n  agents:\n    dest: "OTHER.md"\n' > parent/.ai/src/tools/claude.yaml
    printf '\nshared:\n  path: "parent"\n  inherit: rules, tools\n' >> .ai/agent_sync.yaml
    run run_agentsync sync
    [ "$status" -eq 0 ]
    run run_agentsync check
    [ "$status" -eq 0 ]
    [[ "$output" == *"synced"* ]]
}
```

- [x] **Step 2: Run it, confirm it fails**

Run: `bats tests/check.bats`
Expected: the new test fails at the second `[ "$status" -eq 0 ]`; the other 9 pass.

- [x] **Step 3: Add the helper to `lib/helpers/shared.sh`, after `shared_parent_src`**

```bash
# Echo the CSV of `shared.inherit` categories shared_setup_overlay materialises:
# `subagents` reads as `agents`, and the tokens it warns about are dropped.
# Usage: shared_inherit_categories "<raw inherit value>"
shared_inherit_categories() {
    local raw="${1//,/ }" tok out=""
    for tok in $raw; do
        [[ "$tok" == "subagents" ]] && tok="agents"
        case "$tok" in
            rules|skills|commands|agents) out+="${out:+,}$tok" ;;
        esac
    done
    echo "$out"
}
```

- [x] **Step 4: Use it in `lib/check.sh`**

Replace the line

```bash
        inherit=$(parse_yaml_value "$REPO_ROOT/$config_rel" "shared.inherit")
```

with

```bash
        inherit=$(shared_inherit_categories "$(parse_yaml_value "$REPO_ROOT/$config_rel" "shared.inherit")")
```

- [x] **Step 5: Run the affected files, confirm green**

```bash
bats tests/check.bats tests/shared.bats tests/base_skills.bats
shellcheck -x -S warning -e SC1091 lib/check.sh lib/helpers/shared.sh
```

Expected: 10 + 12 + 12 tests pass; ShellCheck exits 0. The full plan is now `1..747`.

- [x] **Step 6: Commit**

```bash
git add lib/helpers/shared.sh lib/check.sh tests/check.bats
git commit -m "fix(check): inherit only the shared categories sync materialises"
```

---

### Task 1: Filters, Log Voice, and Bash Text Primitives

**Files:**
- Create: `src/filters.rs`
- Create: `src/log.rs`
- Create: `src/text.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `filters::matches(filename: &str, include: &str, exclude: &str) -> bool`, `filters::glob_match(pattern: &str, name: &str) -> bool` — `matches_filter` and `[[ $name == $pat ]]`.
  - `log::Log` (`Default`) with `info`, `success`, `warning`, `error`, `step`, `separator` (all `&mut self, &str` except `separator`), `out(String)`, `err(String)`, `lines() -> &[(Stream, String)]`, `tail(n) -> Vec<&str>`; `log::Stream { Out, Err }`; `log::SEPARATOR`. Plain voice only: the render log is always captured.
  - `text::lines(&[u8]) -> Vec<&[u8]>`, `is_space(u8)`, `trim_start_space`, `trim_end_space`, `after_key(line, key) -> Option<&[u8]>`, `strip_quotes`, `strip_trailing_newlines(&mut Vec<u8>)`, `json_escape(&[u8]) -> Vec<u8>`, `printf_b(&[u8]) -> (Vec<u8>, bool)` (the bool is `\c`).

- [x] **Step 1: Write `src/filters.rs`**

```rust
//! Include/exclude filters, mirroring `matches_filter` in `lib/helpers/filters.sh`.

/// Whether `filename` passes the space-separated glob lists: any exclude match
/// rejects, an empty include accepts everything, otherwise any include match accepts.
pub fn matches(filename: &str, include: &str, exclude: &str) -> bool {
    if split_patterns(exclude).any(|pat| glob_match(pat, filename)) {
        return false;
    }
    if include.is_empty() {
        return true;
    }
    split_patterns(include).any(|pat| glob_match(pat, filename))
}

fn split_patterns(list: &str) -> impl Iterator<Item = &str> {
    list.split([' ', '\t', '\n']).filter(|pat| !pat.is_empty())
}

/// Bash `[[ $name == $pat ]]` matching: `*`, `?`, bracket expressions with `!`
/// or `^` negation and ranges, and backslash escapes. No pathname rules apply,
/// so `*` matches `/` and a leading dot.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = name.chars().collect();
    match_from(&pat, &text)
}

fn match_from(pat: &[char], text: &[char]) -> bool {
    let Some((&first, rest)) = pat.split_first() else {
        return text.is_empty();
    };
    match first {
        '*' => (0..=text.len()).any(|skip| match_from(rest, &text[skip..])),
        '?' => !text.is_empty() && match_from(rest, &text[1..]),
        '[' => match bracket(rest) {
            Some((set, after)) => {
                !text.is_empty() && set.contains(text[0]) && match_from(after, &text[1..])
            }
            None => text.first() == Some(&'[') && match_from(rest, &text[1..]),
        },
        '\\' if !rest.is_empty() => {
            text.first() == Some(&rest[0]) && match_from(&rest[1..], &text[1..])
        }
        literal => text.first() == Some(&literal) && match_from(rest, &text[1..]),
    }
}

struct Bracket {
    negated: bool,
    items: Vec<(char, char)>,
}

impl Bracket {
    fn contains(&self, c: char) -> bool {
        let hit = self.items.iter().any(|&(lo, hi)| lo <= c && c <= hi);
        hit != self.negated
    }
}

/// Parses the body after `[`; `None` when there is no closing `]`, in which
/// case the `[` is literal.
fn bracket(body: &[char]) -> Option<(Bracket, &[char])> {
    let mut i = 0;
    let negated = matches!(body.first(), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let mut items = Vec::new();
    let start = i;
    while i < body.len() {
        let c = body[i];
        if c == ']' && i > start {
            return Some((Bracket { negated, items }, &body[i + 1..]));
        }
        let lo = if c == '\\' && i + 1 < body.len() {
            i += 1;
            body[i]
        } else {
            c
        };
        if i + 2 < body.len() && body[i + 1] == '-' && body[i + 2] != ']' {
            items.push((lo, body[i + 2]));
            i += 3;
        } else {
            items.push((lo, lo));
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_filter_accepts_everything() {
        assert!(matches("core.md", "", ""));
    }

    #[test]
    fn exclude_wins_over_include() {
        assert!(!matches("core.md", "*.md", "core.*"));
        assert!(matches("git.md", "*.md", "core.*"));
    }

    #[test]
    fn any_of_several_space_separated_patterns_matches() {
        assert!(matches("b.md", "a.md b.md", ""));
        assert!(!matches("c.md", "a.md\tb.md", ""));
        assert!(!matches("command-review", "", "legacy command-*"));
    }

    #[test]
    fn globs_follow_bash_pattern_rules() {
        assert!(glob_match("*", ".hidden"));
        assert!(glob_match("a?c", "abc"));
        assert!(glob_match("[a-c]x", "bx"));
        assert!(!glob_match("[!a-c]x", "bx"));
        assert!(glob_match("[x", "[x"));
        assert!(glob_match("\\*", "*"));
        assert!(!glob_match("\\*", "a"));
        assert!(glob_match("*.md", "dir/a.md"));
    }
}
```

- [x] **Step 2: Write `src/log.rs`**

```rust
//! The engine's log voice, mirroring `lib/helpers/logging.sh` without colour:
//! `check` captures the render log the way `lib/check.sh` captured `sync.sh`
//! into a file, where `_use_colors` is false.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

#[derive(Debug, Default)]
pub struct Log {
    lines: Vec<(Stream, String)>,
}

pub const SEPARATOR: &str = "═══════════════════════════════════════════════════════════════";

impl Log {
    pub fn info(&mut self, msg: &str) {
        self.out(format!("[INFO] {msg}"));
    }

    pub fn success(&mut self, msg: &str) {
        self.out(format!("[SUCCESS] {msg}"));
    }

    pub fn warning(&mut self, msg: &str) {
        self.out(format!("[WARNING] {msg}"));
    }

    pub fn error(&mut self, msg: &str) {
        self.err(format!("[ERROR] {msg}"));
    }

    pub fn step(&mut self, msg: &str) {
        self.out(format!("   📁 {msg}"));
    }

    pub fn separator(&mut self) {
        self.out(SEPARATOR.to_string());
    }

    pub fn out(&mut self, line: String) {
        self.lines.push((Stream::Out, line));
    }

    pub fn err(&mut self, line: String) {
        self.lines.push((Stream::Err, line));
    }

    pub fn lines(&self) -> &[(Stream, String)] {
        &self.lines
    }

    /// The last `n` lines of both streams in the order they were written, as
    /// `tail -n` shows a `>file 2>&1` capture.
    pub fn tail(&self, n: usize) -> Vec<&str> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines[skip..]
            .iter()
            .map(|(_, line)| line.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prefixes_match_logging_sh_without_a_terminal() {
        let mut log = Log::default();
        log.info("a");
        log.warning("b");
        log.error("c");
        log.step("d");
        log.success("e");
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[INFO] a",
                "[WARNING] b",
                "[ERROR] c",
                "   📁 d",
                "[SUCCESS] e"
            ]
        );
        assert_eq!(log.lines()[2].0, Stream::Err);
    }

    #[test]
    fn the_separator_is_sixty_three_box_characters() {
        assert_eq!(SEPARATOR.chars().count(), 63);
    }

    #[test]
    fn tail_keeps_the_last_lines_in_write_order() {
        let mut log = Log::default();
        for i in 0..5 {
            log.out(i.to_string());
        }
        assert_eq!(log.tail(2), ["3", "4"]);
        assert_eq!(log.tail(40).len(), 5);
    }
}
```

- [x] **Step 3: Write `src/text.rs`**

```rust
//! Byte-level string handling the Bash engine gets from its builtins: `read`
//! loops, `[[:space:]]`, `printf '%b'`, and `$(...)` newline stripping.

/// Lines as `while IFS= read -r line || [[ -n "$line" ]]` yields them: split on
/// `\n`, with a final unterminated line kept.
pub fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if out.last().is_some_and(|last| last.is_empty()) {
        out.pop();
    }
    out
}

/// POSIX `[[:space:]]` in the C locale.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

pub fn trim_start_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !is_space(*b))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub fn trim_end_space(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| !is_space(*b))
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

/// `rest` of `line` when it starts with `key:` followed by optional spaces,
/// as the Bash `^key:[[:space:]]*(.*)` captures it.
pub fn after_key<'a>(line: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let rest = line.strip_prefix(key.as_bytes())?.strip_prefix(b":")?;
    Some(trim_start_space(rest))
}

/// `${v#\"}; ${v%\"}; ${v#\'}; ${v%\'}`: one quote of each kind off each end.
pub fn strip_quotes(value: &[u8]) -> &[u8] {
    let mut v = value;
    for quote in *b"\"'" {
        v = v.strip_prefix(&[quote]).unwrap_or(v);
        v = v.strip_suffix(&[quote]).unwrap_or(v);
    }
    v
}

/// `$(...)` drops every trailing newline.
pub fn strip_trailing_newlines(bytes: &mut Vec<u8>) {
    while bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
}

/// `_json_escape` and `_toml_escape`: backslash, quote, `\n`, `\r`, `\t`.
pub fn json_escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Bash 3.2 `printf '%b'`. Returns the expansion and whether `\c` stopped
/// all further output.
pub fn printf_b(input: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] != b'\\' || i + 1 >= input.len() {
            out.push(input[i]);
            i += 1;
            continue;
        }
        let next = input[i + 1];
        i += 2;
        let simple = match next {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'e' | b'E' => Some(0x1b),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            b'\\' => Some(b'\\'),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            continue;
        }
        match next {
            b'c' => return (out, true),
            b'0'..=b'7' => {
                let (max, mut value) = if next == b'0' {
                    (3, 0u32)
                } else {
                    (2, u32::from(next - b'0'))
                };
                let mut taken = 0;
                while taken < max && i < input.len() && (b'0'..=b'7').contains(&input[i]) {
                    value = value * 8 + u32::from(input[i] - b'0');
                    i += 1;
                    taken += 1;
                }
                out.push((value & 0xff) as u8);
            }
            b'x' if i < input.len() && input[i].is_ascii_hexdigit() => {
                let mut value = 0u32;
                let mut taken = 0;
                while taken < 2 && i < input.len() && input[i].is_ascii_hexdigit() {
                    value = value * 16 + (input[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    taken += 1;
                }
                out.push(value as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_an_unterminated_last_line() {
        assert_eq!(lines(b"a\nb"), [&b"a"[..], b"b"]);
        assert_eq!(lines(b"a\n\n"), [&b"a"[..], b""]);
        assert!(lines(b"").is_empty());
    }

    #[test]
    fn quotes_come_off_one_of_each_kind_per_end() {
        assert_eq!(strip_quotes(b"\"'x'\""), b"x");
        assert_eq!(strip_quotes(b"\"x"), b"x");
    }

    #[test]
    fn printf_b_expands_like_bash_3_2() {
        assert_eq!(printf_b(b"---\\nk: v\\n---").0, b"---\nk: v\n---");
        assert_eq!(printf_b(b"\\x41\\0101\\101\\e\\z").0, b"AAA\x1b\\z");
        assert_eq!(printf_b(b"\\u0041").0, b"\\u0041");
        assert_eq!(printf_b(b"a\\cb"), (b"a".to_vec(), true));
        assert_eq!(printf_b(b"\\08").0, b"\x008");
    }

    #[test]
    fn json_escape_covers_the_bash_set_only() {
        assert_eq!(
            json_escape(b"a\"b\\c\nd\te\x01"),
            b"a\\\"b\\\\c\\nd\\te\x01"
        );
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
pub mod filters;
pub mod log;
pub mod payload;
pub mod project;
pub mod style;
pub mod text;
pub mod tool;
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
```

- [x] **Step 5: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `46 passed` (35 + 4 `filters` + 3 `log` + 4 `text`).

- [x] **Step 6: Cross-check the glob and `%b` rules against Bash 3.2**

```bash
/bin/bash -c 'f(){ [[ $1 == $2 ]] && echo y || echo n; }; f "[x" "[x"; f "*" "\\*"; f bx "[!a-c]x"; f dir/a.md "*.md"; f .hidden "*"'
/bin/bash -c 'printf "%b|" "\\x41" "\\0101" "\\101" "\\u0041" "\\e[" "\\z" "\\08"; echo' | od -c | head -3
```

Expected: `y`, `y`, `n`, `y`, `y` on five lines, then an `od` dump reading `A | A | A | \ u 0 0 4 1 | 033 [ | \ z | \0 8 |` — the values `glob_match` and `printf_b` assert.

- [x] **Step 7: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/filters.rs src/log.rs src/text.rs src/lib.rs
git commit -m "feat(native): port filters, the log voice, and Bash text primitives"
```

---

### Task 2: Path Rules

**Files:**
- Create: `src/paths.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `log::Log` (Task 1).
- Produces: `paths::ENGINE_ROOT` (`"/<agentsync>"`), `paths::OVERLAY_ROOT` (`"/<agentsync-overlay>"`), `is_virtual(&str)`, `is_within(path, root)`, `parent(&str) -> String`, `leaf(&str) -> String`, `normalize(&str) -> String`, `logical_root(env_root: Option<&str>, cwd: &Path, pwd: Option<&str>) -> String`; `paths::Paths { pub root, pub root_canonical }` with `new(root, root_canonical, home: Option<&str>)`, `for_disk_root(root)`, `absolute(&str)`, `canonicalize_with_existing_ancestor(&str) -> Option<String>`, `resolve_dest(raw, label, &mut Log) -> Option<String>`, `is_safe_source(&str)`, `resolve_source(raw, label, &mut Log) -> Option<String>`, `to_repo_relative(&str) -> Option<String>`, `display(&str) -> String`. `resolve_source` has no engine fallback, matching Task 0b.

- [x] **Step 1: Write `src/paths.rs`**

```rust
//! Path rules of `lib/helpers/paths.sh` and `display_path` from
//! `lib/helpers/logging.sh`, on `/`-separated strings as Bash holds them.

use std::path::Path;

use crate::log::Log;

/// Where `$DEFAULT_REPO_ROOT` points in the native engine: templates are
/// embedded, so the engine checkout is a virtual root served by the workspace.
pub const ENGINE_ROOT: &str = "/<agentsync>";

/// Parent of the virtual trees that replace the Bash overlay tmpdirs.
pub const OVERLAY_ROOT: &str = "/<agentsync-overlay>";

pub fn is_virtual(path: &str) -> bool {
    is_within(path, ENGINE_ROOT) || is_within(path, OVERLAY_ROOT)
}

/// `path` is `root` or lies below it.
pub fn is_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// `_path_parent_r`: `dirname` without the process.
pub fn parent(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let trimmed = trim_trailing_slashes(path);
    if trimmed == "/" {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        Some(idx) => {
            let head = trim_trailing_slashes(&trimmed[..idx]);
            if head.is_empty() {
                "/".to_string()
            } else {
                head.to_string()
            }
        }
    }
}

/// `_path_leaf_r`: `basename` without a suffix argument.
pub fn leaf(path: &str) -> String {
    let trimmed = trim_trailing_slashes(path);
    if trimmed == "/" {
        return "/".to_string();
    }
    trimmed.rsplit('/').next().unwrap_or("").to_string()
}

fn trim_trailing_slashes(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && path.starts_with('/') {
        "/"
    } else {
        trimmed
    }
}

/// Collapses empty, `.` and `..` segments of an absolute path.
pub fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("/{}", segments.join("/"))
}

/// The project root as Bash's `cd "$REPO_ROOT" && pwd` spells it: the logical
/// `$PWD` when it names the working directory, so a symlinked root keeps its
/// spelling in display and manifest paths.
pub fn logical_root(env_root: Option<&str>, cwd: &Path, pwd: Option<&str>) -> String {
    let cwd_text = cwd.to_string_lossy().into_owned();
    let logical_cwd = match pwd {
        Some(pwd)
            if pwd.starts_with('/')
                && std::fs::canonicalize(pwd).ok() == std::fs::canonicalize(cwd).ok() =>
        {
            pwd.to_string()
        }
        _ => cwd_text,
    };
    let base = match env_root {
        Some(root) if root.starts_with('/') => root.to_string(),
        Some(root) => format!("{logical_cwd}/{root}"),
        None => logical_cwd,
    };
    normalize(&base)
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: String,
    pub root_canonical: String,
    home: Option<String>,
}

impl Paths {
    pub fn new(root: &str, root_canonical: &str, home: Option<&str>) -> Self {
        Self {
            root: root.to_string(),
            root_canonical: root_canonical.to_string(),
            home: home.filter(|h| !h.is_empty()).map(str::to_string),
        }
    }

    /// Paths for a root on disk, canonicalised the way `cd -P && pwd` does.
    pub fn for_disk_root(root: &str) -> Self {
        let canonical = std::fs::canonicalize(root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| root.to_string());
        Self::new(root, &canonical, std::env::var("HOME").ok().as_deref())
    }

    /// `normalize_absolute_path_r`: relative paths are taken from the root.
    pub fn absolute(&self, path: &str) -> String {
        if path.starts_with('/') {
            normalize(path)
        } else {
            normalize(&format!("{}/{path}", self.root))
        }
    }

    /// `canonicalize_with_existing_ancestor_r`. Below the root the result is
    /// lexical: `check` renders into a workspace that, like the tar copy Bash
    /// rendered into, has no symlinks under the root. Virtual roots are their
    /// own canonical form; anything else resolves through the disk.
    pub fn canonicalize_with_existing_ancestor(&self, abs: &str) -> Option<String> {
        if is_virtual(abs) {
            return Some(abs.to_string());
        }
        if let Some(rest) = abs.strip_prefix(&self.root)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return Some(format!("{}{rest}", self.root_canonical));
        }
        let mut ancestor = abs.to_string();
        while !Path::new(&ancestor).exists() {
            let up = parent(&ancestor);
            if up == ancestor {
                break;
            }
            ancestor = up;
        }
        let ancestor_canonical = if Path::new(&ancestor).is_dir() {
            canonical_dir(&ancestor)?
        } else {
            format!("{}/{}", canonical_dir(&parent(&ancestor))?, leaf(&ancestor))
        };
        if abs == ancestor {
            return Some(ancestor_canonical);
        }
        let mut suffix = abs[ancestor.len()..].to_string();
        if !suffix.is_empty() && !suffix.starts_with('/') {
            suffix.insert(0, '/');
        }
        Some(normalize(&format!("{ancestor_canonical}{suffix}")))
    }

    /// `resolve_dest_path_r`: the normalised path, or `None` after logging why.
    pub fn resolve_dest(&self, raw: &str, label: &str, log: &mut Log) -> Option<String> {
        if raw.is_empty() {
            log.error(&format!("{label} is empty"));
            return None;
        }
        let abs = self.absolute(raw);
        let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs) else {
            log.error(&format!("Failed to canonicalize {label} path: {raw}"));
            return None;
        };
        if !is_within(&canonical, &self.root_canonical) {
            log.error(&format!(
                "{label} resolves outside repository root: {raw} -> {canonical}"
            ));
            return None;
        }
        Some(abs)
    }

    /// `is_path_safe_source`: the project, the engine, and the overlay trees.
    pub fn is_safe_source(&self, canonical: &str) -> bool {
        is_within(canonical, &self.root_canonical) || is_virtual(canonical)
    }

    /// `resolve_source_path_r`: the normalised path, or `None` after logging an
    /// unsafe root. A missing path is not an error; callers test existence.
    pub fn resolve_source(&self, raw: &str, label: &str, log: &mut Log) -> Option<String> {
        if raw.is_empty() {
            log.error(&format!("{label} is empty"));
            return None;
        }
        let abs = self.absolute(raw);
        if let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs)
            && !self.is_safe_source(&canonical)
        {
            log.error(&format!(
                "{label} resolves outside safe source roots: {raw} -> {canonical}"
            ));
            return None;
        }
        Some(abs)
    }

    /// `to_repo_relative_path_r` without its log line: `.` for the root.
    pub fn to_repo_relative(&self, abs: &str) -> Option<String> {
        if abs == self.root {
            return Some(".".to_string());
        }
        abs.strip_prefix(&self.root)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(str::to_string)
    }

    /// `display_path_r`: root-relative, else `~/`-folded, else unchanged.
    pub fn display(&self, path: &str) -> String {
        if let Some(rel) = self.to_repo_relative(path) {
            return rel;
        }
        if let Some(home) = &self.home
            && let Some(rest) = path
                .strip_prefix(home.as_str())
                .and_then(|r| r.strip_prefix('/'))
        {
            return format!("~/{rest}");
        }
        path.to_string()
    }
}

fn canonical_dir(dir: &str) -> Option<String> {
    std::fs::canonicalize(dir)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths::new("/proj", "/private/proj", Some("/home/me"))
    }

    #[test]
    fn parent_and_leaf_agree_with_dirname_and_basename() {
        let cases = [
            ("/", "/", "/"),
            ("/a", "/", "a"),
            ("/a/", "/", "a"),
            ("/a//b//", "/a", "b"),
            ("a", ".", "a"),
            ("a/b", "a", "b"),
            ("", ".", ""),
        ];
        for (input, dir, base) in cases {
            assert_eq!(parent(input), dir, "dirname {input}");
            assert_eq!(leaf(input), base, "basename {input}");
        }
    }

    #[test]
    fn normalisation_collapses_dot_segments_lexically() {
        assert_eq!(paths().absolute(".claude/./rules/"), "/proj/.claude/rules");
        assert_eq!(paths().absolute("a/../../x"), "/x");
        assert_eq!(paths().absolute("/a//b/.."), "/a");
    }

    #[test]
    fn a_dest_below_the_root_keeps_its_logical_spelling() {
        let mut log = Log::default();
        let dest = paths().resolve_dest(".claude/rules", "targets.rules.dest for Claude", &mut log);
        assert_eq!(dest.as_deref(), Some("/proj/.claude/rules"));
        assert!(log.lines().is_empty());
    }

    #[test]
    fn an_empty_dest_is_logged_and_rejected() {
        let mut log = Log::default();
        assert_eq!(
            paths().resolve_dest("", "targets.rules.dest for X", &mut log),
            None
        );
        assert_eq!(log.tail(1), ["[ERROR] targets.rules.dest for X is empty"]);
    }

    #[test]
    fn sources_in_the_project_the_engine_and_overlays_are_safe() {
        let p = paths();
        assert!(p.is_safe_source("/private/proj/.ai/src/rules"));
        assert!(p.is_safe_source("/<agentsync>/lib/templates/rules"));
        assert!(p.is_safe_source("/<agentsync-overlay>/base-src/src/skills"));
        assert!(!p.is_safe_source("/private/projection"));
        let mut log = Log::default();
        assert_eq!(
            p.resolve_source(".ai/src/missing", "source.rules", &mut log),
            Some("/proj/.ai/src/missing".to_string())
        );
    }

    #[test]
    fn display_paths_strip_the_root_then_fold_home() {
        let p = paths();
        assert_eq!(p.display("/proj/.claude/rules"), ".claude/rules");
        assert_eq!(p.display("/proj"), ".");
        assert_eq!(p.display("/home/me/x"), "~/x");
        assert_eq!(p.display("/<agentsync>/lib"), "/<agentsync>/lib");
        assert_eq!(p.to_repo_relative("/elsewhere"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_dest_outside_the_root_is_rejected_through_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let root = root.to_string_lossy().into_owned();
        let p = Paths::for_disk_root(&root);
        let mut log = Log::default();
        assert_eq!(
            p.resolve_dest("../outside/x", "targets.rules.dest for X", &mut log),
            None
        );
        assert!(log.tail(1)[0].contains("resolves outside repository root: ../outside/x -> "));
    }

    #[cfg(unix)]
    #[test]
    fn the_logical_root_prefers_pwd_when_it_names_the_same_directory() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let link_text = link.to_string_lossy().into_owned();
        assert_eq!(logical_root(None, &real, Some(&link_text)), link_text);
        assert_eq!(
            logical_root(None, &real, Some("/nonexistent")),
            real.to_string_lossy()
        );
        assert_eq!(logical_root(Some("/x/y/.."), &real, None), "/x");
    }
}
```

Below the root, canonicalisation is lexical on purpose: `lib/check.sh` rendered into a `tar` copy, which had no symlinks under its root, so the Bash reference itself never resolved one there. Phase 3's `sync` renders into the real project and extends this for symlinked subdirectories.

- [x] **Step 2: Register the module in `src/lib.rs`**

```rust
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
pub mod yaml_subset;

pub use error::Error;

/// Engine version from the `VERSION` file, the same source `bin/agentsync.sh` reads.
pub fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}
```

- [x] **Step 3: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `54 passed` (46 + 8 `paths`; two of them are `#[cfg(unix)]` and do not run on Windows).

- [x] **Step 4: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/paths.rs src/lib.rs
git commit -m "feat(native): port path normalisation, containment, and display"
```

---

### Task 3: In-Memory Workspace

**Files:**
- Create: `src/workspace.rs`
- Modify: `src/catalog.rs` (add `GLOBAL_CONFIG` and `engine_files`)
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `paths::{ENGINE_ROOT, is_within, is_virtual, parent}` (Task 2), `catalog`.
- Produces: `catalog::GLOBAL_CONFIG: &str` (`lib/config.yaml`), `catalog::engine_files() -> Vec<(String, &'static [u8])>`; `workspace::Content { Disk(PathBuf), Embedded(&'static [u8]), Bytes(Vec<u8>) }`; `workspace::Workspace` with `new(root)` (engine mounted), `root()`, `seed_from_disk(at, disk: &Path, skip: &dyn Fn(&str) -> bool) -> Result<(), Error>`, `insert_file(path, Content)`, `create_dir_all`, `is_file`, `is_dir`, `exists`, `content(&str) -> Option<&Content>`, `read(&str) -> Result<Vec<u8>, Error>`, `list(dir) -> Vec<String>` (byte order, dotfiles), `glob(dir)` (no dotfiles), `files_under(dir)`, `write(path, Vec<u8>)` and `append(path, &[u8])` (parent must exist), `remove` (`rm -rf`), `copy(src, dst)` (`cp -r`).

- [x] **Step 1: Replace `src/catalog.rs`**

```rust
//! Templates shipped with the engine, embedded at build time from `lib/templates/`.

use include_dir::{Dir, File, include_dir};

static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/lib/templates");

/// Shipped `lib/templates/tools/<slug>.yaml`, when the slug is a base tool.
pub fn base_tool_yaml(slug: &str) -> Option<&'static str> {
    TEMPLATES
        .get_file(format!("tools/{slug}.yaml"))?
        .contents_utf8()
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

/// `lib/config.yaml`, the install-dir global config `sync.sh` reads source defaults from.
pub const GLOBAL_CONFIG: &str = include_str!("../lib/config.yaml");

/// Every embedded engine file as its `/`-separated path below the engine root.
pub fn engine_files() -> Vec<(String, &'static [u8])> {
    let mut files = vec![("lib/config.yaml".to_string(), GLOBAL_CONFIG.as_bytes())];
    collect_files(&TEMPLATES, &mut files);
    files
}

fn collect_files(dir: &'static Dir<'static>, out: &mut Vec<(String, &'static [u8])>) {
    for file in dir.files() {
        let rel: Vec<String> = file
            .path()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        out.push((format!("lib/templates/{}", rel.join("/")), file.contents()));
    }
    for sub in dir.dirs() {
        collect_files(sub, out);
    }
}

fn files_in(dir: &str) -> impl Iterator<Item = &'static File<'static>> {
    TEMPLATES
        .get_dir(dir)
        .into_iter()
        .flat_map(|found| found.files())
}

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
        assert_eq!(
            file.path().file_name().and_then(|n| n.to_str()),
            Some("claude.json")
        );
        assert_eq!(base_payload("hooks", "zed").map(|f| f.path()), None);
        assert!(base_payload("hooks", "claude-hub").is_none());
    }
}
```

- [x] **Step 2: Write `src/workspace.rs`**

```rust
//! The file tree a render reads and writes, held in memory.
//!
//! `lib/check.sh` copied `.ai/` and the manifest's outputs into a temporary
//! root with `tar` and ran `sync.sh` there. The workspace is that copy without
//! the copy: paths below the project root and below the virtual engine and
//! overlay roots are served from an index of disk paths, embedded templates,
//! and bytes written by the render. Any other absolute path reads the disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths::{self, ENGINE_ROOT};
use crate::{Error, catalog};

#[derive(Clone, Debug)]
pub enum Content {
    Disk(PathBuf),
    Embedded(&'static [u8]),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
enum Entry {
    Dir,
    File(Content),
}

#[derive(Debug)]
pub struct Workspace {
    root: String,
    entries: BTreeMap<String, Entry>,
}

impl Workspace {
    /// An empty tree rooted at `root`, with the embedded engine mounted.
    pub fn new(root: &str) -> Self {
        let mut ws = Self {
            root: root.to_string(),
            entries: BTreeMap::new(),
        };
        ws.create_dir_all(root);
        for (rel, bytes) in catalog::engine_files() {
            ws.insert_file(&format!("{ENGINE_ROOT}/{rel}"), Content::Embedded(bytes));
        }
        ws
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    fn indexed(&self, path: &str) -> bool {
        paths::is_within(path, &self.root) || paths::is_virtual(path)
    }

    /// Adds the disk tree at `disk` under `at`, skipping every path for which
    /// `skip` returns true, given its `/`-separated path relative to `disk`.
    pub fn seed_from_disk(
        &mut self,
        at: &str,
        disk: &Path,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<(), Error> {
        let meta = std::fs::metadata(disk).map_err(|e| Error::io(disk, e))?;
        if meta.is_file() {
            self.insert_file(at, Content::Disk(disk.to_path_buf()));
            return Ok(());
        }
        self.create_dir_all(at);
        self.seed_dir(at, disk, "", skip)
    }

    fn seed_dir(
        &mut self,
        at: &str,
        disk: &Path,
        rel: &str,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<(), Error> {
        let entries = std::fs::read_dir(disk).map_err(|e| Error::io(disk, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(disk, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if skip(&child_rel) {
                continue;
            }
            let child_disk = entry.path();
            let child_at = format!("{at}/{name}");
            let Ok(meta) = std::fs::metadata(&child_disk) else {
                continue;
            };
            if meta.is_dir() {
                self.entries.insert(child_at.clone(), Entry::Dir);
                self.seed_dir(&child_at, &child_disk, &child_rel, skip)?;
            } else {
                self.entries
                    .insert(child_at, Entry::File(Content::Disk(child_disk)));
            }
        }
        Ok(())
    }

    pub fn insert_file(&mut self, path: &str, content: Content) {
        self.create_dir_all(&paths::parent(path));
        self.entries.insert(path.to_string(), Entry::File(content));
    }

    /// `mkdir -p`.
    pub fn create_dir_all(&mut self, path: &str) {
        let mut current = path.to_string();
        while !matches!(self.entries.get(&current), Some(Entry::Dir)) {
            self.entries.insert(current.clone(), Entry::Dir);
            let up = paths::parent(&current);
            if up == current {
                break;
            }
            current = up;
        }
    }

    pub fn is_file(&self, path: &str) -> bool {
        if self.indexed(path) {
            matches!(self.entries.get(path), Some(Entry::File(_)))
        } else {
            Path::new(path).is_file()
        }
    }

    pub fn is_dir(&self, path: &str) -> bool {
        if self.indexed(path) {
            matches!(self.entries.get(path), Some(Entry::Dir))
        } else {
            Path::new(path).is_dir()
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.is_file(path) || self.is_dir(path)
    }

    pub fn content(&self, path: &str) -> Option<&Content> {
        match self.entries.get(path) {
            Some(Entry::File(content)) => Some(content),
            _ => None,
        }
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        if !self.indexed(path) {
            return std::fs::read(path).map_err(|e| Error::io(path, e));
        }
        match self.entries.get(path) {
            Some(Entry::File(Content::Disk(disk))) => {
                std::fs::read(disk).map_err(|e| Error::io(disk, e))
            }
            Some(Entry::File(Content::Embedded(bytes))) => Ok(bytes.to_vec()),
            Some(Entry::File(Content::Bytes(bytes))) => Ok(bytes.clone()),
            _ => Err(not_found(path)),
        }
    }

    /// Entry names directly inside `dir` in byte order, dotfiles included.
    pub fn list(&self, dir: &str) -> Vec<String> {
        if !self.indexed(dir) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return Vec::new();
            };
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            return names;
        }
        let prefix = if dir == "/" {
            "/".to_string()
        } else {
            format!("{dir}/")
        };
        self.entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .filter_map(|(key, _)| {
                let rest = &key[prefix.len()..];
                (!rest.is_empty() && !rest.contains('/')).then(|| rest.to_string())
            })
            .collect()
    }

    /// Names a Bash `"$dir"/*` glob yields: no dotfiles.
    pub fn glob(&self, dir: &str) -> Vec<String> {
        self.list(dir)
            .into_iter()
            .filter(|name| !name.starts_with('.'))
            .collect()
    }

    /// Regular files below `dir` at any depth, as `find "$dir" -type f` lists them.
    pub fn files_under(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        for name in self.list(dir) {
            let child = format!("{dir}/{name}");
            if self.is_dir(&child) {
                found.extend(self.files_under(&child));
            } else if self.is_file(&child) {
                found.push(child);
            }
        }
        found
    }

    /// `>`: the parent directory must exist.
    pub fn write(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), Error> {
        if !self.is_dir(&paths::parent(path)) || self.is_dir(path) {
            return Err(not_found(path));
        }
        self.entries
            .insert(path.to_string(), Entry::File(Content::Bytes(bytes)));
        Ok(())
    }

    /// `>>`: creates the file when missing, the parent directory must exist.
    pub fn append(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        let mut current = if self.is_file(path) {
            self.read(path)?
        } else {
            Vec::new()
        };
        current.extend_from_slice(bytes);
        self.write(path, current)
    }

    /// `rm -rf`.
    pub fn remove(&mut self, path: &str) {
        let prefix = format!("{path}/");
        let doomed: Vec<String> = self
            .entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .map(|(key, _)| key.clone())
            .collect();
        for key in doomed {
            self.entries.remove(&key);
        }
        self.entries.remove(path);
    }

    /// `cp -r src dst` onto a missing `dst`: a file, or a whole tree with its
    /// empty directories.
    pub fn copy(&mut self, src: &str, dst: &str) -> Result<(), Error> {
        if self.is_file(src) {
            let content = self.content_of(src)?;
            if !self.is_dir(&paths::parent(dst)) {
                return Err(not_found(dst));
            }
            self.entries.insert(dst.to_string(), Entry::File(content));
            return Ok(());
        }
        if !self.is_dir(src) {
            return Err(not_found(src));
        }
        self.create_dir_all(dst);
        for name in self.list(src) {
            self.copy(&format!("{src}/{name}"), &format!("{dst}/{name}"))?;
        }
        Ok(())
    }

    fn content_of(&self, path: &str) -> Result<Content, Error> {
        if !self.indexed(path) {
            return Ok(Content::Disk(PathBuf::from(path)));
        }
        self.content(path).cloned().ok_or_else(|| not_found(path))
    }
}

fn not_found(path: &str) -> Error {
    Error::io(
        path,
        std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        let mut ws = Workspace::new("/proj");
        ws.insert_file(
            "/proj/.ai/src/rules/core.md",
            Content::Bytes(b"# Core\n".to_vec()),
        );
        ws.insert_file("/proj/.ai/src/rules/.hidden.md", Content::Bytes(Vec::new()));
        ws.create_dir_all("/proj/.ai/src/skills/empty");
        ws
    }

    #[test]
    fn inserting_a_file_creates_its_parents() {
        let ws = ws();
        assert!(ws.is_dir("/proj/.ai/src"));
        assert!(ws.is_file("/proj/.ai/src/rules/core.md"));
        assert!(!ws.is_file("/proj/.ai/src/rules"));
    }

    #[test]
    fn listing_is_immediate_children_in_byte_order_and_glob_drops_dotfiles() {
        let ws = ws();
        assert_eq!(ws.list("/proj/.ai/src/rules"), [".hidden.md", "core.md"]);
        assert_eq!(ws.glob("/proj/.ai/src/rules"), ["core.md"]);
        assert_eq!(ws.list("/proj/.ai/src"), ["rules", "skills"]);
    }

    #[test]
    fn write_needs_a_parent_and_append_creates_the_file() {
        let mut ws = ws();
        assert!(ws.write("/proj/missing/x.md", b"x".to_vec()).is_err());
        ws.create_dir_all("/proj/out");
        ws.append("/proj/out/a.md", b"one\n").unwrap();
        ws.append("/proj/out/a.md", b"two\n").unwrap();
        assert_eq!(ws.read("/proj/out/a.md").unwrap(), b"one\ntwo\n");
    }

    #[test]
    fn copy_brings_empty_directories_and_remove_takes_the_subtree() {
        let mut ws = ws();
        ws.copy("/proj/.ai/src", "/proj/copy").unwrap();
        assert!(ws.is_dir("/proj/copy/skills/empty"));
        assert!(ws.is_file("/proj/copy/rules/core.md"));
        assert_eq!(ws.files_under("/proj/copy").len(), 2);
        ws.remove("/proj/copy/rules");
        assert!(!ws.exists("/proj/copy/rules/core.md"));
        assert!(ws.is_dir("/proj/copy/skills"));
    }

    #[test]
    fn the_engine_templates_are_mounted_under_the_virtual_root() {
        let ws = Workspace::new("/proj");
        assert!(ws.is_file("/<agentsync>/lib/templates/settings/claude.json"));
        assert!(ws.is_file("/<agentsync>/lib/config.yaml"));
        assert!(ws.is_dir("/<agentsync>/lib/templates/base-src/skills/agentsync"));
    }

    #[cfg(unix)]
    #[test]
    fn seeding_from_disk_honours_the_skip_filter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/rules")).unwrap();
        std::fs::create_dir_all(dir.path().join("backups/x")).unwrap();
        std::fs::write(dir.path().join("src/rules/core.md"), "c").unwrap();
        let mut ws = Workspace::new("/proj");
        ws.seed_from_disk("/proj/.ai", dir.path(), &|rel| rel == "backups")
            .unwrap();
        assert_eq!(ws.read("/proj/.ai/src/rules/core.md").unwrap(), b"c");
        assert!(!ws.exists("/proj/.ai/backups"));
    }
}
```

- [x] **Step 3: Register the module in `src/lib.rs`**

```rust
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
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `60 passed` (54 + 6 `workspace`).

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/workspace.rs src/catalog.rs src/lib.rs
git commit -m "feat(native): hold the render workspace in memory"
```

---

### Task 4: Frontmatter and Per-File Converters

**Files:**
- Create: `src/convert.rs`
- Modify: `src/lib.rs`
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` ("Known quirks", item 9)

**Interfaces:**
- Consumes: `text` (Task 1).
- Produces: `convert::Frontmatter { name, description, model, tools: Vec<Vec<u8>>, tools_declared, readonly, body }` (all `pub`, bytes); `parse_frontmatter(&[u8]) -> Frontmatter` (`_parse_md_frontmatter`); `read_field(source, field) -> Vec<u8>` (`read_frontmatter_field`); `replace_all(line, from, to) -> Vec<u8>`; `command_to_toml(source)`, `agent_to_toml(stem, source)`, `agent_to_amazonq_json(stem, source)`, `agent_to_opencode_md(stem, source)`, `command_to_skill(name, source)` — each `-> Vec<u8>`, the file the Bash converter writes.

- [x] **Step 1: Write `src/convert.rs`**

```rust
//! Markdown frontmatter and the per-file converters of
//! `lib/helpers/format_conversion.sh`: Gemini command TOML, Codex agent TOML,
//! Amazon Q agent JSON, and OpenCode agent Markdown.

use crate::text::{self, after_key, is_space, strip_quotes};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub name: Vec<u8>,
    pub description: Vec<u8>,
    pub model: Vec<u8>,
    pub tools: Vec<Vec<u8>>,
    pub tools_declared: bool,
    pub readonly: Vec<u8>,
    pub body: Vec<u8>,
}

/// `_parse_md_frontmatter`.
pub fn parse_frontmatter(source: &[u8]) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut in_frontmatter = false;
    let mut done = false;
    let mut in_multiline_desc = false;
    let mut in_tools_list = false;

    for line in text::lines(source) {
        if !done {
            if line == b"---" && !in_frontmatter {
                in_frontmatter = true;
                continue;
            }
            if line == b"---" {
                done = true;
                in_multiline_desc = false;
                in_tools_list = false;
                continue;
            }
            if in_frontmatter {
                parse_frontmatter_line(line, &mut fm, &mut in_multiline_desc, &mut in_tools_list);
                continue;
            }
            done = true;
        }
        fm.body.extend_from_slice(line);
        fm.body.push(b'\n');
    }
    fm
}

fn parse_frontmatter_line(
    line: &[u8],
    fm: &mut Frontmatter,
    in_multiline_desc: &mut bool,
    in_tools_list: &mut bool,
) {
    if *in_multiline_desc {
        if line.first().is_some_and(|b| is_space(*b)) {
            let cont = text::trim_start_space(line);
            if !fm.description.is_empty() {
                fm.description.push(b' ');
            }
            fm.description.extend_from_slice(cont);
            return;
        }
        *in_multiline_desc = false;
    }
    if *in_tools_list {
        if let Some(item) = block_list_item(line) {
            fm.tools.push(strip_quotes(item).to_vec());
            return;
        }
        *in_tools_list = false;
    }

    if let Some(rest) = after_key(line, "name") {
        fm.name = strip_quotes(rest).to_vec();
    } else if let Some(rest) = after_key(line, "model") {
        fm.model = strip_quotes(rest).to_vec();
    } else if let Some(inner) = after_key(line, "tools").and_then(inline_list) {
        fm.tools_declared = true;
        for item in inner.split(|b| *b == b',') {
            let item = item.strip_prefix(b" ").unwrap_or(item);
            let item = item.strip_suffix(b" ").unwrap_or(item);
            let item = strip_quotes(item);
            if !item.is_empty() {
                fm.tools.push(item.to_vec());
            }
        }
    } else if after_key(line, "tools").is_some_and(<[u8]>::is_empty) {
        fm.tools_declared = true;
        *in_tools_list = true;
    } else if let Some(rest) = after_key(line, "readonly") {
        fm.readonly = strip_quotes(rest).to_vec();
    } else if after_key(line, "description").is_some_and(|rest| rest == b">") {
        *in_multiline_desc = true;
        fm.description.clear();
    } else if let Some(rest) = after_key(line, "description") {
        fm.description = strip_quotes(rest).to_vec();
    }
}

/// `^[[:space:]]+-[[:space:]]+(.*)`.
fn block_list_item(line: &[u8]) -> Option<&[u8]> {
    let after_indent = text::trim_start_space(line);
    if after_indent.len() == line.len() {
        return None;
    }
    let after_dash = after_indent.strip_prefix(b"-")?;
    let item = text::trim_start_space(after_dash);
    (item.len() < after_dash.len()).then_some(item)
}

/// `\[(.*)\][[:space:]]*$` on the text after `tools:`.
fn inline_list(rest: &[u8]) -> Option<&[u8]> {
    let trimmed = text::trim_end_space(rest);
    if trimmed.len() >= 2 && trimmed.starts_with(b"[") && trimmed.ends_with(b"]") {
        Some(&trimmed[1..trimmed.len() - 1])
    } else {
        None
    }
}

/// `read_frontmatter_field`: the last value of `field` inside a leading
/// `---` block, unquoted, cut at `#`, right-trimmed.
pub fn read_field(source: &[u8], field: &str) -> Vec<u8> {
    let mut in_frontmatter = false;
    let mut value: Vec<u8> = Vec::new();
    for line in text::lines(source) {
        if !in_frontmatter {
            if line != b"---" {
                return Vec::new();
            }
            in_frontmatter = true;
            continue;
        }
        if line == b"---" {
            return value;
        }
        if let Some(rest) = after_key(line, field) {
            let unquoted = strip_quotes(rest);
            let cut = unquoted
                .iter()
                .position(|b| *b == b'#')
                .map_or(unquoted, |idx| &unquoted[..idx]);
            value = text::trim_end_space(cut).to_vec();
        }
    }
    value
}

/// Per-line `sed` substitution: every non-overlapping `from` becomes `to`.
pub fn replace_all(line: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(line[i]);
            i += 1;
        }
    }
    out
}

/// `sed 's/!`\([^`]*\)`/!{\1}/g'` on one line.
fn braces_for_shell_sugar(line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i..].starts_with(b"!`")
            && let Some(close) = line[i + 2..].iter().position(|b| *b == b'`')
        {
            out.extend_from_slice(b"!{");
            out.extend_from_slice(&line[i + 2..i + 2 + close]);
            out.push(b'}');
            i += close + 3;
            continue;
        }
        out.push(line[i]);
        i += 1;
    }
    out
}

/// `$(echo "$text" | sed …)`: a per-line rewrite with trailing newlines dropped.
fn sed_lines(input: &[u8], rewrite: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut with_newline = input.to_vec();
    with_newline.push(b'\n');
    let mut out = Vec::with_capacity(with_newline.len());
    for line in text::lines(&with_newline) {
        out.extend(rewrite(line));
        out.push(b'\n');
    }
    text::strip_trailing_newlines(&mut out);
    out
}

/// `convert_md_command_to_toml`.
pub fn command_to_toml(source: &[u8]) -> Vec<u8> {
    let fm = parse_frontmatter(source);
    let body = sed_lines(&fm.body, braces_for_shell_sugar);
    let body = sed_lines(&body, |line| replace_all(line, b"$ARGUMENTS", b"{{args}}"));
    let mut out = Vec::new();
    if !fm.description.is_empty() {
        out.extend_from_slice(b"description = \"");
        out.extend(text::json_escape(&fm.description));
        out.extend_from_slice(b"\"\n");
    }
    out.extend_from_slice(b"prompt = \"\"\"\n");
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\"\"\"\n");
    out
}

/// `convert_md_agent_to_toml`.
pub fn agent_to_toml(stem: &str, source: &[u8]) -> Vec<u8> {
    let fm = parse_frontmatter(source);
    let name = if fm.name.is_empty() {
        stem.as_bytes()
    } else {
        &fm.name
    };
    let mut out = Vec::new();
    out.extend_from_slice(b"name = \"");
    out.extend(text::json_escape(name));
    out.extend_from_slice(b"\"\n");
    if !fm.description.is_empty() {
        out.extend_from_slice(b"description = \"");
        out.extend(text::json_escape(&fm.description));
        out.extend_from_slice(b"\"\n");
    }
    out.extend_from_slice(b"developer_instructions = \"\"\"\n");
    out.extend_from_slice(&fm.body);
    out.extend_from_slice(b"\"\"\"\n");
    out
}

/// `convert_md_agent_to_amazonq_json`.
pub fn agent_to_amazonq_json(stem: &str, source: &[u8]) -> Vec<u8> {
    let fm = parse_frontmatter(source);
    let name = if fm.name.is_empty() {
        stem.as_bytes()
    } else {
        &fm.name
    };
    let body = fm.body.strip_suffix(b"\n").unwrap_or(&fm.body);
    let mut tools = b"[".to_vec();
    for (i, tool) in fm.tools.iter().filter(|t| !t.is_empty()).enumerate() {
        if i > 0 {
            tools.extend_from_slice(b", ");
        }
        tools.push(b'"');
        tools.extend(text::json_escape(tool));
        tools.push(b'"');
    }
    tools.push(b']');

    let mut out = b"{\n  \"name\": \"".to_vec();
    out.extend(text::json_escape(name));
    out.extend_from_slice(b"\",\n");
    if !fm.description.is_empty() {
        out.extend_from_slice(b"  \"description\": \"");
        out.extend(text::json_escape(&fm.description));
        out.extend_from_slice(b"\",\n");
    }
    if !fm.model.is_empty() {
        out.extend_from_slice(b"  \"model\": \"");
        out.extend(text::json_escape(&fm.model));
        out.extend_from_slice(b"\",\n");
    }
    out.extend_from_slice(b"  \"tools\": ");
    out.extend(tools);
    out.extend_from_slice(b",\n  \"mcpServers\": {},\n  \"prompt\": \"");
    out.extend(text::json_escape(body));
    out.extend_from_slice(b"\"\n}\n");
    out
}

/// `convert_md_agent_to_opencode_md`.
pub fn agent_to_opencode_md(stem: &str, source: &[u8]) -> Vec<u8> {
    let fm = parse_frontmatter(source);
    let description: &[u8] = if !fm.description.is_empty() {
        &fm.description
    } else if !fm.name.is_empty() {
        &fm.name
    } else {
        stem.as_bytes()
    };

    let mut permissions = Vec::new();
    if fm.tools_declared {
        permissions.extend_from_slice(b"  \"*\": deny\n");
        let mut seen: Vec<&str> = Vec::new();
        for tool in &fm.tools {
            let mapped = match tool.as_slice() {
                b"Read" => "read",
                b"Grep" => "grep",
                b"Glob" => "glob",
                b"Bash" => "bash",
                b"Write" | b"Edit" => "edit",
                b"WebFetch" => "webfetch",
                b"WebSearch" => "websearch",
                b"Task" => "task",
                _ => continue,
            };
            if seen.contains(&mapped) {
                continue;
            }
            permissions.extend_from_slice(format!("  \"{mapped}\": allow\n").as_bytes());
            seen.push(mapped);
        }
    } else if fm.readonly == b"true" {
        permissions.extend_from_slice(b"  edit: deny\n  bash: deny\n");
    }

    let mut out = b"---\n".to_vec();
    if !description.is_empty() {
        out.extend_from_slice(b"description: \"");
        out.extend(text::json_escape(description));
        out.extend_from_slice(b"\"\n");
    }
    out.extend_from_slice(b"mode: subagent\n");
    if fm.model.contains(&b'/') {
        out.extend_from_slice(b"model: \"");
        out.extend(text::json_escape(&fm.model));
        out.extend_from_slice(b"\"\n");
    }
    if !permissions.is_empty() {
        out.extend_from_slice(b"permission:\n");
        out.extend(permissions);
    }
    out.extend_from_slice(b"---\n");
    out.extend_from_slice(&fm.body);
    out
}

/// The generated `SKILL.md` of `sync_commands_as_skills` for `<name>.md`.
pub fn command_to_skill(name: &str, source: &[u8]) -> Vec<u8> {
    let mut description = read_field(source, "description");
    if description.is_empty() {
        description = format!("Run the /{name} command workflow.").into_bytes();
    }

    let mut body = Vec::new();
    let mut in_fm = false;
    let mut fm_seen = false;
    let mut body_started = false;
    for (index, line) in text::lines(source).into_iter().enumerate() {
        if index == 0 && line == b"---" {
            in_fm = true;
            continue;
        }
        if in_fm && line == b"---" {
            in_fm = false;
            fm_seen = true;
            continue;
        }
        if in_fm || (!body_started && line.is_empty() && fm_seen) {
            continue;
        }
        body_started = true;
        let line = replace_all(line, b"$ARGUMENTS", b"<arg>");
        body.extend(replace_all(&line, b"!`", b"`"));
        body.push(b'\n');
    }
    text::strip_trailing_newlines(&mut body);

    let mut out = format!("---\nname: \"command-{name}\"\ndescription: >-\n  ").into_bytes();
    out.extend(description);
    out.extend_from_slice(b"\n---\n\n");
    out.extend(body);
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &[u8] = b"---\nname: \"code-reviewer\"\ndescription: >\n  Reviews code\n  carefully\ntools:\n  - Read\n  - \"Grep\"\nmodel: sonnet\n---\n\nYou review.\n";

    #[test]
    fn frontmatter_folds_a_multiline_description_and_collects_block_tools() {
        let fm = parse_frontmatter(AGENT);
        assert_eq!(fm.name, b"code-reviewer");
        assert_eq!(fm.description, b"Reviews code carefully");
        assert_eq!(fm.tools, [b"Read".to_vec(), b"Grep".to_vec()]);
        assert!(fm.tools_declared);
        assert_eq!(fm.model, b"sonnet");
        assert_eq!(fm.body, b"\nYou review.\n");
    }

    #[test]
    fn inline_tools_trim_one_space_and_quotes_per_item() {
        let fm = parse_frontmatter(b"---\ntools: [Read, 'Glob',, Bash ]\n---\nx\n");
        assert_eq!(
            fm.tools,
            [b"Read".to_vec(), b"Glob".to_vec(), b"Bash".to_vec()]
        );
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let fm = parse_frontmatter(b"# Title\n---\n");
        assert_eq!(fm.body, b"# Title\n---\n");
        assert!(fm.name.is_empty());
    }

    // Design spec, "Known quirks", item 9: reproduced on purpose until cutover.
    #[test]
    fn read_field_takes_the_last_occurrence_like_bash_does() {
        let src = b"---\ndescription: first\ndescription: \"second\" # note\n---\n";
        assert_eq!(read_field(src, "description"), b"second\"");
        assert_eq!(read_field(b"# no fm\n", "description"), b"");
    }

    #[test]
    fn command_toml_rewrites_shell_sugar_and_drops_trailing_newlines() {
        let src = b"---\ndescription: Say \"hi\"\n---\nRun !`git status` for $ARGUMENTS\n\n";
        assert_eq!(
            String::from_utf8(command_to_toml(src)).unwrap(),
            "description = \"Say \\\"hi\"\nprompt = \"\"\"\nRun !{git status} for {{args}}\"\"\"\n"
        );
    }

    #[test]
    fn agent_toml_keeps_the_body_newline() {
        assert_eq!(
            agent_to_toml("x", b"body\n"),
            b"name = \"x\"\ndeveloper_instructions = \"\"\"\nbody\n\"\"\"\n"
        );
    }

    #[test]
    fn amazonq_json_escapes_the_prompt() {
        let out = String::from_utf8(agent_to_amazonq_json("code-reviewer", AGENT)).unwrap();
        assert_eq!(
            out,
            "{\n  \"name\": \"code-reviewer\",\n  \"description\": \"Reviews code carefully\",\n  \"model\": \"sonnet\",\n  \"tools\": [\"Read\", \"Grep\"],\n  \"mcpServers\": {},\n  \"prompt\": \"\\nYou review.\"\n}\n"
        );
    }

    #[test]
    fn opencode_md_maps_tools_to_an_allowlist_and_omits_portable_models() {
        let out = String::from_utf8(agent_to_opencode_md("code-reviewer", AGENT)).unwrap();
        assert_eq!(
            out,
            "---\ndescription: \"Reviews code carefully\"\nmode: subagent\npermission:\n  \"*\": deny\n  \"read\": allow\n  \"grep\": allow\n---\n\nYou review.\n"
        );
        let readonly = String::from_utf8(agent_to_opencode_md(
            "r",
            b"---\nreadonly: true\nmodel: a/b\n---\n",
        ))
        .unwrap();
        assert_eq!(
            readonly,
            "---\ndescription: \"r\"\nmode: subagent\nmodel: \"a/b\"\npermission:\n  edit: deny\n  bash: deny\n---\n"
        );
    }

    #[test]
    fn command_skill_skips_blank_lines_after_frontmatter() {
        let src = b"---\ndescription: Review it\n---\n\n\nCheck $ARGUMENTS with !`ls`\n";
        assert_eq!(
            String::from_utf8(command_to_skill("review", src)).unwrap(),
            "---\nname: \"command-review\"\ndescription: >-\n  Review it\n---\n\nCheck <arg> with `ls`\n"
        );
        assert!(
            String::from_utf8(command_to_skill("x", b"body"))
                .unwrap()
                .contains("  Run the /x command workflow.\n")
        );
    }
}
```

- [x] **Step 2: Register the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod convert;
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
```

- [x] **Step 3: Record the quirk in the design spec**

Under "Known quirks to reproduce now and fix after cutover", after item 8, add:

```markdown
9. `read_frontmatter_field` returns the last occurrence of a key, although its
   comment promises the first.
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `69 passed` (60 + 9 `convert`).

- [x] **Step 5: Cross-check the converters against Bash**

```bash
dir=$(mktemp -d "${TMPDIR:-/tmp}/conv.XXXXXX")
printf -- '---\ndescription: Say "hi"\n---\nRun !`git status` for $ARGUMENTS\n\n' > "$dir/cmd.md"
printf -- '---\nname: "code-reviewer"\ndescription: >\n  Reviews code\n  carefully\ntools:\n  - Read\n  - "Grep"\nmodel: sonnet\n---\n\nYou review.\n' > "$dir/code-reviewer.md"
bash -c 'source lib/helpers/logging.sh; source lib/helpers/file_ops.sh; source lib/helpers/format_conversion.sh
  convert_md_command_to_toml "$1/cmd.md" "$1/cmd.toml"; printf "%q\n" "$(cat "$1/cmd.toml"; echo x)"
  convert_md_agent_to_opencode_md "$1/code-reviewer.md" "$1/cr.md"; printf "%q\n" "$(cat "$1/cr.md"; echo x)"' _ "$dir"
```

Expected, the bytes `command_toml_rewrites_shell_sugar_and_drops_trailing_newlines` and `opencode_md_maps_tools_to_an_allowlist_and_omits_portable_models` assert:

```text
$'description = "Say \\"hi"\nprompt = """\nRun !{git status} for {{args}}"""\nx'
$'---\ndescription: "Reviews code carefully"\nmode: subagent\npermission:\n  "*": deny\n  "read": allow\n  "grep": allow\n---\n\nYou review.\nx'
```

- [x] **Step 6: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/convert.rs src/lib.rs docs/specs/2026-09-12-rust-migration-design.md
git commit -m "feat(native): port frontmatter parsing and the file converters"
```

---

### Task 5: OpenCode JSON Composition

**Files:**
- Create: `src/opencode_json.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `opencode_json::compose(settings: &str, mcp: &str) -> Result<String, ComposeError>`; `ComposeError { pub code: u8, pub message: String }` — the awk exit code (20–26) and the diagnostic line `sync_opencode_config` logs.

- [x] **Step 1: Write `src/opencode_json.rs`**

```rust
//! `opencode.json` composition: settings plus the canonical MCP source,
//! ported from the awk program in `lib/helpers/opencode.sh`. The first
//! failure wins and later parsing continues, exactly as awk's `fail()` did.

#[derive(Debug, PartialEq, Eq)]
pub struct ComposeError {
    pub code: u8,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Settings,
    Canonical,
    Server,
}

#[derive(Default)]
struct Members {
    keys: Vec<String>,
    raw_keys: Vec<String>,
    values: Vec<String>,
}

struct Parser {
    json: Vec<char>,
    pos: usize,
    active_error: u8,
    error: Option<(u8, String)>,
    last_string: String,
    settings: Members,
    canonical: Members,
    server: Members,
}

/// awk's `getline` loop: every line, the last one included, ends in `\n`.
fn read_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

fn json_kind(raw: &str) -> &'static str {
    let trimmed = raw.trim_start_matches(is_space);
    match trimmed.chars().next() {
        Some('"') => "string",
        Some('{') => "object",
        Some('[') => "array",
        _ => {
            let word = trimmed.trim_end_matches(is_space);
            if word == "true" || word == "false" {
                "boolean"
            } else if word == "null" {
                "null"
            } else {
                "number"
            }
        }
    }
}

fn is_word_char(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
}

fn is_timeout(value: &str) -> bool {
    let v: Vec<char> = value.trim_matches(is_space).chars().collect();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < v.len() && v[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if !digits(&mut i) {
        return false;
    }
    if i < v.len() && v[i] == '.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if i < v.len() && (v[i] == 'e' || v[i] == 'E') {
        i += 1;
        if i < v.len() && (v[i] == '+' || v[i] == '-') {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == v.len()
}

impl Parser {
    fn new() -> Self {
        Self {
            json: Vec::new(),
            pos: 0,
            active_error: 0,
            error: None,
            last_string: String::new(),
            settings: Members::default(),
            canonical: Members::default(),
            server: Members::default(),
        }
    }

    fn fail(&mut self, code: u8, message: impl Into<String>) -> bool {
        if self.error.is_none() {
            self.error = Some((code, message.into()));
        }
        false
    }

    fn peek(&self) -> Option<char> {
        self.json.get(self.pos).copied()
    }

    fn slice(&self, start: usize) -> String {
        self.json[start..self.pos.min(self.json.len())]
            .iter()
            .collect()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(is_space) {
            self.pos += 1;
        }
    }

    fn parse_string(&mut self) -> bool {
        let code = self.active_error;
        if self.peek() != Some('"') {
            return self.fail(code, "expected a JSON string");
        }
        self.pos += 1;
        let mut decoded = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.pos += 1;
                self.last_string = decoded;
                return true;
            }
            if c == '\\' {
                self.pos += 1;
                let Some(escape) = self.peek() else {
                    return self.fail(code, "unterminated JSON escape");
                };
                if escape == 'u' {
                    let hex: String = self.json.iter().skip(self.pos + 1).take(4).collect();
                    if hex.chars().count() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                        return self.fail(code, "invalid Unicode escape");
                    }
                    let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
                    if value <= 127 {
                        decoded.push(char::from_u32(value).unwrap_or('\0'));
                    } else {
                        decoded.push_str("\\u");
                        decoded.push_str(&hex);
                    }
                    self.pos += 5;
                    continue;
                }
                let mapped = match escape {
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' | '\\' | '/' => escape,
                    _ => return self.fail(code, "invalid JSON escape"),
                };
                decoded.push(mapped);
                self.pos += 1;
                continue;
            }
            if c.is_ascii_control() {
                return self.fail(code, "control character in JSON string");
            }
            decoded.push(c);
            self.pos += 1;
        }
        self.fail(code, "unterminated JSON string")
    }

    fn parse_array(&mut self) -> bool {
        let code = self.active_error;
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some(']') {
            self.pos += 1;
            return true;
        }
        while self.pos < self.json.len() {
            if !self.parse_value() {
                return false;
            }
            self.skip_space();
            match self.peek() {
                Some(']') => {
                    self.pos += 1;
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON array"),
            }
        }
        self.fail(code, "unterminated JSON array")
    }

    fn parse_object(&mut self) -> bool {
        let code = self.active_error;
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            return true;
        }
        while self.pos < self.json.len() {
            if !self.parse_string() {
                return false;
            }
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(code, "expected colon in JSON object");
            }
            self.pos += 1;
            self.skip_space();
            if !self.parse_value() {
                return false;
            }
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON object"),
            }
        }
        self.fail(code, "unterminated JSON object")
    }

    fn parse_value(&mut self) -> bool {
        self.skip_space();
        match self.peek() {
            Some('"') => return self.parse_string(),
            Some('{') => return self.parse_object(),
            Some('[') => return self.parse_array(),
            _ => {}
        }
        for word in ["true", "false", "null"] {
            let len = word.len();
            let matches = self
                .json
                .get(self.pos..self.pos + len)
                .is_some_and(|s| s.iter().copied().eq(word.chars()));
            if matches && !is_word_char(self.json.get(self.pos + len)) {
                self.pos += len;
                return true;
            }
        }
        if let Some(len) = self.number_len() {
            self.pos += len;
            return true;
        }
        let code = self.active_error;
        self.fail(code, "invalid JSON value")
    }

    /// Length of `^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?` at the cursor.
    fn number_len(&self) -> Option<usize> {
        let s = &self.json[self.pos..];
        let digit = |i: usize| s.get(i).is_some_and(|c| c.is_ascii_digit());
        let mut i = usize::from(s.first() == Some(&'-'));
        match s.get(i) {
            Some('0') => i += 1,
            Some(c) if c.is_ascii_digit() => {
                while digit(i) {
                    i += 1;
                }
            }
            _ => return None,
        }
        if s.get(i) == Some(&'.') && digit(i + 1) {
            i += 1;
            while digit(i) {
                i += 1;
            }
        }
        if matches!(s.get(i), Some('e' | 'E')) {
            let mut j = i + 1;
            if matches!(s.get(j), Some('+' | '-')) {
                j += 1;
            }
            if digit(j) {
                while digit(j) {
                    j += 1;
                }
                i = j;
            }
        }
        Some(i)
    }

    fn store_member(&mut self, mode: Mode, key: String, raw_key: String, raw_value: String) {
        let active = self.active_error;
        let (members, code, label) = match mode {
            Mode::Settings => (&self.settings, 20, "duplicate settings field"),
            Mode::Canonical => (&self.canonical, active, "duplicate canonical MCP field"),
            Mode::Server => (&self.server, 23, "duplicate server field"),
        };
        if members.keys.contains(&key) {
            self.fail(code, format!("{label} '{key}'"));
            return;
        }
        let members = match mode {
            Mode::Settings => &mut self.settings,
            Mode::Canonical => &mut self.canonical,
            Mode::Server => &mut self.server,
        };
        members.keys.push(key);
        members.raw_keys.push(raw_key);
        members.values.push(raw_value);
    }

    fn walk_root(&mut self, text: &str, mode: Mode, code: u8) -> bool {
        self.json = text.chars().collect();
        self.pos = 0;
        self.active_error = code;
        self.skip_space();
        if self.peek() != Some('{') {
            return self.fail(code, "root value must be an object");
        }
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            self.skip_space();
            return self.pos >= self.json.len();
        }
        while self.pos < self.json.len() {
            let key_start = self.pos;
            if !self.parse_string() {
                return false;
            }
            let key = self.last_string.clone();
            let raw_key = self.slice(key_start);
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(code, "expected colon after object key");
            }
            self.pos += 1;
            self.skip_space();
            let value_start = self.pos;
            if !self.parse_value() {
                return false;
            }
            let raw_value = self.slice(value_start);
            self.store_member(mode, key, raw_key, raw_value);
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    self.skip_space();
                    if self.pos < self.json.len() {
                        return self.fail(code, "trailing content after JSON object");
                    }
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON object"),
            }
        }
        self.fail(code, "unterminated JSON object")
    }

    fn decode_string(&mut self, raw: &str) -> String {
        let saved = (std::mem::take(&mut self.json), self.pos, self.active_error);
        self.json = raw.chars().collect();
        self.pos = 0;
        self.active_error = 23;
        let result = if self.parse_string() {
            self.last_string.clone()
        } else {
            String::new()
        };
        (self.json, self.pos, self.active_error) = saved;
        result
    }

    fn validate_string_array(&mut self, raw: &str, server: &str, field: &str) -> bool {
        self.json = raw.chars().collect();
        self.pos = 1;
        self.active_error = 23;
        self.skip_space();
        if self.peek() == Some(']') {
            return true;
        }
        let invalid = format!("server '{server}' field '{field}' is invalid");
        while self.pos < self.json.len() {
            let start = self.pos;
            if !self.parse_value() {
                return false;
            }
            if json_kind(&self.slice(start)) != "string" {
                return self.fail(
                    23,
                    format!("server '{server}' field '{field}' must contain only strings"),
                );
            }
            self.skip_space();
            match self.peek() {
                Some(']') => return true,
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(23, invalid),
            }
        }
        self.fail(23, invalid)
    }

    fn validate_string_map(&mut self, raw: &str, server: &str, field: &str) -> bool {
        self.json = raw.chars().collect();
        self.pos = 1;
        self.active_error = 23;
        self.skip_space();
        if self.peek() == Some('}') {
            return true;
        }
        let invalid = format!("server '{server}' field '{field}' is invalid");
        while self.pos < self.json.len() {
            if !self.parse_string() {
                return false;
            }
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(23, invalid);
            }
            self.pos += 1;
            self.skip_space();
            let value_start = self.pos;
            if !self.parse_value() {
                return false;
            }
            if json_kind(&self.slice(value_start)) != "string" {
                return self.fail(
                    23,
                    format!("server '{server}' field '{field}' values must be strings"),
                );
            }
            self.skip_space();
            match self.peek() {
                Some('}') => return true,
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(23, invalid),
            }
        }
        self.fail(23, invalid)
    }

    fn convert_server(&mut self, name: &str, raw_value: &str) -> Option<String> {
        self.server = Members::default();
        if !self.walk_root(raw_value, Mode::Server, 23) {
            return None;
        }
        let fields: Vec<(String, String)> = self
            .server
            .keys
            .iter()
            .cloned()
            .zip(self.server.values.iter().cloned())
            .collect();
        let mut command = String::new();
        let mut url = String::new();
        let mut type_ = String::new();
        let mut args = String::new();
        let mut env = String::new();
        let mut headers = String::new();
        let mut enabled = String::new();
        let mut timeout = String::new();
        let mut oauth = String::new();
        let must =
            |field: &str, what: &str| format!("server '{name}' field '{field}' must be {what}");
        for (field, value) in fields {
            let kind = json_kind(&value);
            match field.as_str() {
                "command" | "url" => {
                    if kind != "string" {
                        self.fail(23, must(&field, "a string"));
                        return None;
                    }
                    if field == "command" {
                        command = value;
                    } else {
                        url = value;
                    }
                }
                "args" => {
                    if kind != "array" {
                        self.fail(23, must("args", "an array"));
                        return None;
                    }
                    if !self.validate_string_array(&value, name, "args") {
                        return None;
                    }
                    args = value;
                }
                "env" | "headers" => {
                    if kind != "object" {
                        self.fail(23, must(&field, "an object"));
                        return None;
                    }
                    if !self.validate_string_map(&value, name, &field) {
                        return None;
                    }
                    if field == "env" {
                        env = value;
                    } else {
                        headers = value;
                    }
                }
                "type" => {
                    if kind != "string" {
                        self.fail(23, must("type", "a string"));
                        return None;
                    }
                    type_ = self.decode_string(&value);
                }
                "enabled" => {
                    if kind != "boolean" {
                        self.fail(23, must("enabled", "a boolean"));
                        return None;
                    }
                    enabled = value;
                }
                "timeout" => {
                    if kind != "number" || !is_timeout(&value) {
                        self.fail(23, must("timeout", "a non-negative number"));
                        return None;
                    }
                    timeout = value;
                }
                "oauth" => {
                    if kind != "boolean" && kind != "object" {
                        self.fail(23, must("oauth", "a boolean or object"));
                        return None;
                    }
                    oauth = value;
                }
                other => {
                    self.fail(
                        25,
                        format!("server '{name}' has unsupported field '{other}'"),
                    );
                    return None;
                }
            }
        }
        if command.is_empty() == url.is_empty() {
            self.fail(
                26,
                format!("server '{name}' must define exactly one transport: command or url"),
            );
            return None;
        }
        let mut props: Vec<String> = Vec::new();
        if !command.is_empty() {
            if !headers.is_empty() || !oauth.is_empty() {
                self.fail(25, format!("server '{name}' contains remote-only fields"));
                return None;
            }
            if !type_.is_empty() && type_ != "stdio" {
                self.fail(
                    23,
                    format!("server '{name}' field 'type' must be stdio for a local server"),
                );
                return None;
            }
            props.push("\"type\": \"local\"".to_string());
            let inner = array_inner(&args);
            let value = if inner.is_empty() {
                format!("[{command}]")
            } else {
                format!("[{command}, {inner}]")
            };
            props.push(format!("\"command\": {value}"));
            if !env.is_empty() {
                props.push(format!("\"environment\": {env}"));
            }
        } else {
            if !args.is_empty() || !env.is_empty() {
                self.fail(25, format!("server '{name}' contains local-only fields"));
                return None;
            }
            if !type_.is_empty() && !matches!(type_.as_str(), "http" | "sse" | "streamable-http") {
                self.fail(
                    23,
                    format!("server '{name}' field 'type' is not a supported remote transport"),
                );
                return None;
            }
            props.push("\"type\": \"remote\"".to_string());
            props.push(format!("\"url\": {url}"));
            if !headers.is_empty() {
                props.push(format!("\"headers\": {headers}"));
            }
            if !oauth.is_empty() {
                props.push(format!("\"oauth\": {oauth}"));
            }
        }
        if !enabled.is_empty() {
            props.push(format!("\"enabled\": {enabled}"));
        }
        if !timeout.is_empty() {
            props.push(format!("\"timeout\": {timeout}"));
        }
        Some(format!("{{{}}}", props.join(", ")))
    }

    fn take_error(&mut self) -> Option<ComposeError> {
        self.error
            .take()
            .map(|(code, message)| ComposeError { code, message })
    }
}

/// awk `sub(/^[[:space:]]*\[[[:space:]]*/)` and `sub(/[[:space:]]*\][[:space:]]*$/)`.
fn array_inner(raw: &str) -> String {
    let mut value = raw;
    let lead = value.trim_start_matches(is_space);
    if let Some(rest) = lead.strip_prefix('[') {
        value = rest.trim_start_matches(is_space);
    }
    let tail = value.trim_end_matches(is_space);
    if let Some(rest) = tail.strip_suffix(']') {
        value = rest.trim_end_matches(is_space);
    }
    value.to_string()
}

/// The composed `opencode.json` bytes, or the exit code and diagnostic line
/// `_opencode_compose_json` reported.
pub fn compose(settings: &str, mcp: &str) -> Result<String, ComposeError> {
    let mut p = Parser::new();
    let settings_text = read_text(settings);
    if !p.walk_root(&settings_text, Mode::Settings, 20) && p.error.is_none() {
        p.fail(20, "malformed settings JSON");
    }
    if let Some(error) = p.take_error() {
        return Err(error);
    }
    let settings_has_mcp = p.settings.keys.iter().any(|k| k == "mcp");

    let mcp_text = read_text(mcp);
    if !p.walk_root(&mcp_text, Mode::Canonical, 21) && p.error.is_none() {
        p.fail(21, "malformed canonical MCP JSON");
    }
    if let Some(error) = p.take_error() {
        return Err(error);
    }
    if settings_has_mcp {
        return Err(ComposeError {
            code: 24,
            message: "settings and canonical source both define OpenCode MCP ownership".into(),
        });
    }

    let mut mcp_servers = String::new();
    for (key, value) in p.canonical.keys.iter().zip(&p.canonical.values) {
        if key == "mcpServers" {
            mcp_servers = value.clone();
        } else {
            return Err(ComposeError {
                code: 25,
                message: format!("canonical MCP has unsupported top-level field '{key}'"),
            });
        }
    }
    if mcp_servers.is_empty() || json_kind(&mcp_servers) != "object" {
        return Err(ComposeError {
            code: 22,
            message: "mcpServers must be an object".into(),
        });
    }

    p.canonical = Members::default();
    if !p.walk_root(&mcp_servers, Mode::Canonical, 23) {
        let (code, message) = p.error.take().unwrap_or((0, String::new()));
        return Err(ComposeError { code, message });
    }
    let servers: Vec<(String, String, String)> = p
        .canonical
        .keys
        .iter()
        .cloned()
        .zip(p.canonical.raw_keys.iter().cloned())
        .zip(p.canonical.values.iter().cloned())
        .map(|((k, r), v)| (k, r, v))
        .collect();
    let mut converted = Vec::with_capacity(servers.len());
    for (key, raw_key, value) in &servers {
        if json_kind(value) != "object" {
            return Err(ComposeError {
                code: 23,
                message: format!("server '{key}' must be an object"),
            });
        }
        let server = p.convert_server(key, value);
        if let Some(error) = p.take_error() {
            return Err(error);
        }
        converted.push((raw_key.clone(), server.unwrap_or_default()));
    }

    let mut out = String::from("{\n");
    for (raw_key, value) in p.settings.raw_keys.iter().zip(&p.settings.values) {
        out.push_str(&format!("  {raw_key}: {value},\n"));
    }
    out.push_str("  \"mcp\": {\n");
    let count = converted.len();
    for (i, (raw_key, server)) in converted.into_iter().enumerate() {
        let comma = if i + 1 < count { "," } else { "" };
        out.push_str(&format!("    {raw_key}: {server}{comma}\n"));
    }
    out.push_str("  }\n}\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(settings: &str, mcp: &str) -> (u8, String) {
        let e = compose(settings, mcp).unwrap_err();
        (e.code, e.message)
    }

    #[test]
    fn a_local_server_is_composed_after_the_settings_members() {
        let out = compose(
            "{\"$schema\":\"https://opencode.ai/config.json\",\"theme\":\"system\"}\n",
            "{\"mcpServers\":{\"github\":{\"command\":\"npx\",\"args\":[\"-y\",\"@github/mcp\"],\"env\":{\"TOKEN\":\"${GITHUB_TOKEN}\"}}}}\n",
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"theme\": \"system\",\n  \"mcp\": {\n    \"github\": {\"type\": \"local\", \"command\": [\"npx\", \"-y\",\"@github/mcp\"], \"environment\": {\"TOKEN\":\"${GITHUB_TOKEN}\"}}\n  }\n}\n"
        );
    }

    #[test]
    fn a_remote_server_keeps_its_options() {
        let out = compose(
            "{}",
            "{\"mcpServers\":{\"r\":{\"url\":\"https://x\",\"type\":\"sse\",\"headers\":{},\"oauth\":false,\"enabled\":true,\"timeout\":5}}}",
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  \"mcp\": {\n    \"r\": {\"type\": \"remote\", \"url\": \"https://x\", \"headers\": {}, \"oauth\": false, \"enabled\": true, \"timeout\": 5}\n  }\n}\n"
        );
    }

    #[test]
    fn failures_carry_the_awk_exit_codes() {
        assert_eq!(err("[]", "{}"), (20, "root value must be an object".into()));
        assert_eq!(
            err("{\"a\":1,\"a\":2}", "{}"),
            (20, "duplicate settings field 'a'".into())
        );
        assert_eq!(
            err("{}", "{\"mcpServers\":[]}"),
            (22, "mcpServers must be an object".into())
        );
        assert_eq!(
            err("{}", "{\"mcpServers\":{\"s\":1}}"),
            (23, "server 's' must be an object".into())
        );
        assert_eq!(
            err("{\"mcp\":{}}", "{\"mcpServers\":{}}"),
            (
                24,
                "settings and canonical source both define OpenCode MCP ownership".into()
            )
        );
        assert_eq!(
            err("{}", "{\"x\":1}"),
            (
                25,
                "canonical MCP has unsupported top-level field 'x'".into()
            )
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"url\":\"b\"}}}"
            ),
            (
                26,
                "server 's' must define exactly one transport: command or url".into()
            )
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"cwd\":\"b\"}}}"
            ),
            (25, "server 's' has unsupported field 'cwd'".into())
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"args\":[1]}}}"
            ),
            (
                23,
                "server 's' field 'args' must contain only strings".into()
            )
        );
        assert_eq!(err("{} x", "{}"), (20, "malformed settings JSON".into()));
        assert_eq!(
            err("{\"a\":01}", "{}"),
            (20, "expected comma in JSON object".into())
        );
    }

    #[test]
    fn unicode_escapes_above_ascii_stay_escaped_in_decoded_keys() {
        let out = compose("{\"k\\u00e9\":\"\\u0041\"}", "{\"mcpServers\":{}}").unwrap();
        assert_eq!(
            out,
            "{\n  \"k\\u00e9\": \"\\u0041\",\n  \"mcp\": {\n  }\n}\n"
        );
    }
}
```

- [x] **Step 2: Register the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod filters;
pub mod log;
pub mod opencode_json;
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
```

- [x] **Step 3: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `73 passed` (69 + 4 `opencode_json`).

- [x] **Step 4: Cross-check two failures against the awk program**

```bash
dir=$(mktemp -d "${TMPDIR:-/tmp}/oc.XXXXXX")
printf '{"a":01}\n' > "$dir/s.json"; printf '{}\n' > "$dir/m.json"
bash -c 'source lib/helpers/logging.sh; source lib/helpers/tmp.sh; source lib/helpers/opencode.sh; tmp_prime_run_dir
  rc=0; _opencode_compose_json "$1/s.json" "$1/m.json" "$1/o.json" || rc=$?; echo "$rc $OPENCODE_JSON_DIAGNOSTIC"
  printf "{}\n" > "$1/s.json"; printf "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"cwd\":\"b\"}}}\n" > "$1/m.json"
  rc=0; _opencode_compose_json "$1/s.json" "$1/m.json" "$1/o.json" || rc=$?; echo "$rc $OPENCODE_JSON_DIAGNOSTIC"; tmp_cleanup' _ "$dir"
```

Expected: `20 expected comma in JSON object` and `25 server 's' has unsupported field 'cwd'`, as `failures_carry_the_awk_exit_codes` asserts.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/opencode_json.rs src/lib.rs
git commit -m "feat(native): compose opencode.json from settings and canonical MCP"
```

---

### Task 6: Render Session, File Operations, and Rule Directories

**Files:**
- Create: `src/session.rs`
- Create: `src/file_ops.rs`
- Create: `src/rules.rs`
- Modify: `src/lib.rs`
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` ("Known quirks", item 10)

**Interfaces:**
- Consumes: `Workspace` (Task 3), `Paths` (Task 2), `Log`, `filters`, `text` (Task 1), `convert` (Task 4).
- Produces:
  - `session::Session { pub ws: Workspace, pub paths: Paths, pub log: Log }` with `new(ws, paths)`, `display(&str)`, `record_write(&str)`, `record_tree(&str)`, `was_touched(&str) -> bool`, `touched() -> &BTreeSet<String>`, `warn_legacy_payload(&str)`; `#[cfg(test)] session::test_session() -> Session` rooted at `/proj`.
  - `file_ops::cleanup_path(&mut Session, target) -> bool`, `copy_file(&mut Session, src, dest) -> Result<(), Error>`, `sync_dir(&mut Session, src, dest, include, exclude) -> Result<(), Error>`.
  - `rules::add_header`, `merge_or_prepend_header`, `rule_paths_csv`, `strip_frontmatter`, `apply_rule_header` (pure, bytes); `append_imports(s, agents_file, rules_dir)`, `merge_rules_to_file(s, src_dir, dest_file, include, exclude, agents_file: Option<&str>)`, `sync_rules(s, src_dir, dest_dir, &RuleOptions)`, `inline_commands_to_file(s, src_dir, target_file, include, exclude)`, `sync_commands_as_skills(s, src_dir, dest_dir, include, exclude)`, `sync_converted(s, src_dir, dest_dir, Conversion)` — each `-> Result<(), Error>`; `RuleOptions<'a> { extension, header, scoped_header, include, exclude }`; `Conversion { CommandToml, AgentToml, AgentAmazonqJson, AgentOpencodeMd }`.
  - All sweeps prune as a forced run does: `sync_may_prune` is always true under `check`'s `--force`, so the preserve branch waits for Phase 3.

- [x] **Step 1: Write `src/session.rs`**

```rust
//! State one render shares across its steps: the workspace, path rules, the
//! log, and the manifest's record of what this run wrote (`manifest.sh`).

use std::collections::BTreeSet;

use crate::log::Log;
use crate::paths::Paths;
use crate::workspace::Workspace;

pub struct Session {
    pub ws: Workspace,
    pub paths: Paths,
    pub log: Log,
    touched: BTreeSet<String>,
    legacy_payload_warned: bool,
}

impl Session {
    pub fn new(ws: Workspace, paths: Paths) -> Self {
        Self {
            ws,
            paths,
            log: Log::default(),
            touched: BTreeSet::new(),
            legacy_payload_warned: false,
        }
    }

    pub fn display(&self, path: &str) -> String {
        self.paths.display(path)
    }

    /// `manifest_record_write`: paths outside the root are ignored silently.
    pub fn record_write(&mut self, abs: &str) {
        if let Some(rel) = self.paths.to_repo_relative(abs) {
            self.touched.insert(rel);
        }
    }

    /// `manifest_record_tree`.
    pub fn record_tree(&mut self, dir: &str) {
        for file in self.ws.files_under(dir) {
            self.record_write(&file);
        }
    }

    /// `manifest_was_touched`.
    pub fn was_touched(&self, abs: &str) -> bool {
        self.paths
            .to_repo_relative(abs)
            .is_some_and(|rel| self.touched.contains(&rel))
    }

    pub fn touched(&self) -> &BTreeSet<String> {
        &self.touched
    }

    /// `_warn_legacy_payload_path`: once per run, on stderr.
    pub fn warn_legacy_payload(&mut self, abs: &str) {
        if self.legacy_payload_warned {
            return;
        }
        self.legacy_payload_warned = true;
        let root_prefix = format!("{}/", self.paths.root);
        let rel = abs.strip_prefix(&root_prefix).unwrap_or(abs).to_string();
        self.log
            .err(format!("⚠  Legacy payload override layout detected: {rel}"));
        self.log
            .err("   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).".into());
        self.log
            .err("   Migrate with: agentsync migrate --legacy".into());
    }
}

#[cfg(test)]
pub(crate) fn test_session() -> Session {
    Session::new(Workspace::new("/proj"), Paths::new("/proj", "/proj", None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_recorded_relative_to_the_root_and_outside_paths_are_ignored() {
        let mut s = test_session();
        s.record_write("/proj/CLAUDE.md");
        s.record_write("/elsewhere/x");
        assert!(s.was_touched("/proj/CLAUDE.md"));
        assert_eq!(s.touched().len(), 1);
    }

    #[test]
    fn the_legacy_warning_prints_once() {
        let mut s = test_session();
        s.warn_legacy_payload("/proj/.ai/src/mcp/claude.json");
        s.warn_legacy_payload("/proj/.ai/src/mcp/cursor.json");
        assert_eq!(s.log.lines().len(), 3);
        assert_eq!(
            s.log.tail(3)[0],
            "⚠  Legacy payload override layout detected: .ai/src/mcp/claude.json"
        );
    }
}
```

- [x] **Step 2: Write `src/file_ops.rs`**

```rust
//! `lib/helpers/file_ops.sh` for a forced render: every extraneous entry a
//! sweep owns is pruned, as `sync_may_prune` allows under `--force`.

use crate::session::Session;
use crate::{Error, filters, paths};

/// `cleanup_path`: removes `target` when it exists; true when something went.
pub fn cleanup_path(s: &mut Session, target: &str) -> bool {
    if !s.ws.exists(target) {
        return false;
    }
    s.ws.remove(target);
    let shown = s.display(target);
    s.log.step(&format!("Removed: {shown}"));
    true
}

/// `copy_file`: a missing source is a warning, not a failure.
pub fn copy_file(s: &mut Session, src: &str, dest: &str) -> Result<(), Error> {
    if !s.ws.is_file(src) {
        s.log.warning(&format!("Source file not found: {src}"));
        return Ok(());
    }
    let src_disp = s.display(src);
    let dest_disp = s.display(dest);
    s.ws.create_dir_all(&paths::parent(dest));
    if s.ws.is_dir(dest) {
        s.ws.copy(src, &format!("{dest}/{}", paths::leaf(src)))?;
    } else {
        s.ws.remove(dest);
        s.ws.copy(src, dest)?;
    }
    s.record_write(dest);
    s.log.step(&format!("{src_disp} → {dest_disp}"));
    Ok(())
}

/// `sync_dir`: copy every filtered top-level entry, then prune entries the
/// filter owns that the source no longer has and this run did not write.
pub fn sync_dir(
    s: &mut Session,
    src: &str,
    dest: &str,
    include: &str,
    exclude: &str,
) -> Result<(), Error> {
    if !s.ws.is_dir(src) {
        s.log.warning(&format!("Source directory not found: {src}"));
        return Ok(());
    }
    let src_disp = s.display(src);
    let dest_disp = s.display(dest);
    s.ws.create_dir_all(dest);

    let mut source_items: Vec<String> = Vec::new();
    for name in s.ws.glob(src) {
        if !filters::matches(&name, include, exclude) {
            continue;
        }
        let target = format!("{dest}/{name}");
        s.ws.remove(&target);
        s.ws.copy(&format!("{src}/{name}"), &target)?;
        if s.ws.is_dir(&target) {
            s.record_tree(&target);
        } else {
            s.record_write(&target);
        }
        source_items.push(name);
    }

    let mut cleaned = 0usize;
    for name in s.ws.glob(dest) {
        if source_items.contains(&name) || !filters::matches(&name, include, exclude) {
            continue;
        }
        let item = format!("{dest}/{name}");
        if s.was_touched(&item) {
            continue;
        }
        s.ws.remove(&item);
        s.log.step(&format!("Removed: {dest_disp}/{name}"));
        cleaned += 1;
    }

    let extra = if include.is_empty() {
        String::new()
    } else {
        format!(", include='{include}'")
    };
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ ({} updates, {cleaned} cleanups){extra}",
        source_items.len()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    #[test]
    fn copy_file_replaces_the_dest_and_records_it() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "new");
        file(&mut s, "/proj/CLAUDE.md", "old");
        copy_file(&mut s, "/proj/.ai/src/AGENTS.md", "/proj/CLAUDE.md").unwrap();
        assert_eq!(s.ws.read("/proj/CLAUDE.md").unwrap(), b"new");
        assert!(s.was_touched("/proj/CLAUDE.md"));
        assert_eq!(s.log.tail(1), ["   📁 .ai/src/AGENTS.md → CLAUDE.md"]);
    }

    #[test]
    fn copy_file_warns_on_a_missing_source() {
        let mut s = test_session();
        copy_file(&mut s, "/proj/nope.json", "/proj/.mcp.json").unwrap();
        assert_eq!(
            s.log.tail(1),
            ["[WARNING] Source file not found: /proj/nope.json"]
        );
        assert!(!s.ws.exists("/proj/.mcp.json"));
    }

    #[test]
    fn sync_dir_copies_trees_and_prunes_only_what_the_filter_owns() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.ai/src/skills/a/references/r.md", "r");
        file(&mut s, "/proj/.ai/src/skills/.hidden/SKILL.md", "h");
        file(&mut s, "/proj/.claude/skills/stale/SKILL.md", "s");
        file(&mut s, "/proj/.claude/skills/command-review/SKILL.md", "c");
        sync_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            "",
            "command-*",
        )
        .unwrap();
        assert!(s.ws.is_file("/proj/.claude/skills/a/references/r.md"));
        assert!(!s.ws.exists("/proj/.claude/skills/.hidden"));
        assert!(!s.ws.exists("/proj/.claude/skills/stale"));
        assert!(s.ws.exists("/proj/.claude/skills/command-review"));
        assert!(s.was_touched("/proj/.claude/skills/a/references/r.md"));
        assert_eq!(
            s.log.tail(2),
            [
                "   📁 Removed: .claude/skills/stale",
                "   📁 .ai/src/skills/ → .claude/skills/ (1 updates, 1 cleanups)"
            ]
        );
    }

    #[test]
    fn cleanup_path_reports_whether_anything_was_removed() {
        let mut s = test_session();
        file(&mut s, "/proj/.cursor/rules/core.mdc", "x");
        assert!(cleanup_path(&mut s, "/proj/.cursor/rules"));
        assert!(!cleanup_path(&mut s, "/proj/.cursor/rules"));
    }
}
```

- [x] **Step 3: Write `src/rules.rs`**

```rust
//! Rule, command, and subagent directory operations of
//! `lib/helpers/rule_operations.sh` and the directory loops of
//! `lib/helpers/format_conversion.sh`, for a forced render.

use crate::session::Session;
use crate::{Error, convert, filters, paths, text};

/// `add_header`: `printf '%b\n'` of the header, a blank line, the file.
pub fn add_header(file: &[u8], header: &str) -> Vec<u8> {
    let (mut out, stopped) = text::printf_b(header.as_bytes());
    if !stopped {
        out.push(b'\n');
    }
    out.push(b'\n');
    out.extend_from_slice(file);
    out
}

fn frontmatter_key(line: &[u8]) -> Option<&[u8]> {
    let first = *line.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let end = line
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'))?;
    (line[end] == b':').then_some(&line[..end])
}

/// `merge_or_prepend_header`: a file without frontmatter gets the header; a
/// file with frontmatter gains only the header keys it lacks.
pub fn merge_or_prepend_header(file: &[u8], header: &str) -> Vec<u8> {
    let lines = text::lines(file);
    if lines.first().copied().unwrap_or(b"") != b"---" {
        return add_header(file, header);
    }
    let mut existing: Vec<&[u8]> = Vec::new();
    let mut in_fm = false;
    for line in &lines {
        if *line == b"---" {
            if in_fm {
                break;
            }
            in_fm = true;
            continue;
        }
        if in_fm && let Some(key) = frontmatter_key(line) {
            existing.push(key);
        }
    }
    let (expanded, _) = text::printf_b(header.as_bytes());
    let mut additions = Vec::new();
    for hline in text::lines(&expanded) {
        if hline == b"---" || hline.is_empty() {
            continue;
        }
        if let Some(key) = frontmatter_key(hline)
            && !existing.contains(&key)
        {
            additions.extend_from_slice(hline);
            additions.push(b'\n');
        }
    }
    if additions.is_empty() {
        return file.to_vec();
    }
    let mut out = Vec::with_capacity(file.len() + additions.len());
    let mut fences = 0;
    for line in lines {
        if line == b"---" {
            fences += 1;
            if fences == 2 {
                out.extend_from_slice(&additions);
            }
        }
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

/// `_rule_paths_csv`: every list item in the leading frontmatter, joined with
/// commas, when that frontmatter has a bare `paths:` key.
pub fn rule_paths_csv(file: &[u8]) -> Vec<u8> {
    let lines = text::lines(file);
    if lines.first().copied() != Some(b"---".as_slice()) {
        return Vec::new();
    }
    let mut block: Vec<&[u8]> = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(1) {
        block.push(line);
        if index > 1 && *line == b"---" {
            break;
        }
    }
    let has_paths = block
        .iter()
        .any(|line| text::after_key(line, "paths").is_some_and(<[u8]>::is_empty));
    if !has_paths {
        return Vec::new();
    }
    let mut items: Vec<Vec<u8>> = Vec::new();
    for line in block {
        let rest = text::trim_start_space(line);
        let Some(after_dash) = rest.strip_prefix(b"-") else {
            continue;
        };
        if !after_dash.first().is_some_and(|b| text::is_space(*b)) {
            continue;
        }
        let mut item = text::trim_start_space(after_dash);
        if let Some(first) = item.first()
            && (*first == b'"' || *first == b'\'')
        {
            item = &item[1..];
        }
        if let Some(last) = item.last()
            && (*last == b'"' || *last == b'\'')
        {
            item = &item[..item.len() - 1];
        }
        items.push(item.to_vec());
    }
    items.join(&b","[..])
}

/// `_strip_frontmatter`: the leading `---` block and the blank lines after it.
pub fn strip_frontmatter(file: &[u8]) -> Vec<u8> {
    let lines = text::lines(file);
    if lines.first().copied() != Some(b"---".as_slice()) {
        return file.to_vec();
    }
    let close = lines.iter().skip(1).position(|l| *l == b"---");
    let Some(close) = close.map(|i| i + 1) else {
        return Vec::new();
    };
    let rest = &lines[close + 1..];
    let first_content = rest
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(rest.len());
    let kept = &rest[first_content..];
    let mut out = Vec::new();
    for (i, line) in kept.iter().enumerate() {
        out.extend_from_slice(line);
        if i + 1 < kept.len() || file.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    out
}

/// `apply_rule_header`: a `paths:`-scoped rule takes the scoped header with
/// `{globs}` filled; otherwise the always-on header is merged in.
pub fn apply_rule_header(file: &[u8], header: &str, scoped_header: &str) -> Vec<u8> {
    let globs = rule_paths_csv(file);
    if !globs.is_empty() && !scoped_header.is_empty() {
        let globs = String::from_utf8_lossy(&globs);
        return add_header(
            &strip_frontmatter(file),
            &scoped_header.replace("{globs}", &globs),
        );
    }
    if !header.is_empty() {
        return merge_or_prepend_header(file, header);
    }
    file.to_vec()
}

fn md_files(s: &Session, dir: &str) -> Vec<String> {
    s.ws.glob(dir)
        .into_iter()
        .filter(|name| name.ends_with(".md") && s.ws.is_file(&format!("{dir}/{name}")))
        .collect()
}

fn read(s: &Session, path: &str) -> Result<Vec<u8>, Error> {
    s.ws.read(path)
}

/// `append_imports`.
pub fn append_imports(s: &mut Session, agents_file: &str, rules_dir: &str) -> Result<(), Error> {
    if !s.ws.is_file(agents_file) {
        s.log
            .warning(&format!("Agents file not found: {agents_file}"));
        return Ok(());
    }
    if !s.ws.is_dir(rules_dir) {
        s.log.warning(&format!(
            "Rules directory not found for imports: {rules_dir}"
        ));
        return Ok(());
    }
    let mut block = b"\n<!-- Auto-generated imports -->\n".to_vec();
    for name in md_files(s, rules_dir) {
        block.extend_from_slice(format!("@rules/{name}\n").as_bytes());
    }
    s.ws.append(agents_file, &block)?;
    s.record_write(agents_file);
    Ok(())
}

/// `merge_rules_to_file`.
pub fn merge_rules_to_file(
    s: &mut Session,
    src_dir: &str,
    dest_file: &str,
    include: &str,
    exclude: &str,
    agents_file: Option<&str>,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        s.log.warning(&format!("Rules source not found: {src_dir}"));
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_file);
    let files: Vec<String> = md_files(s, src_dir)
        .into_iter()
        .filter(|name| filters::matches(name, include, exclude))
        .collect();

    s.ws.create_dir_all(&paths::parent(dest_file));
    s.ws.remove(dest_file);
    if let Some(agents) = agents_file.filter(|a| s.ws.is_file(a)) {
        let mut preamble = read(s, agents)?;
        preamble.extend_from_slice(b"\n---\n\n");
        s.ws.append(dest_file, &preamble)?;
    }
    for (i, name) in files.iter().enumerate() {
        let mut chunk = if i == 0 {
            Vec::new()
        } else {
            b"\n---\n\n".to_vec()
        };
        chunk.extend(read(s, &format!("{src_dir}/{name}"))?);
        s.ws.append(dest_file, &chunk)?;
    }
    s.record_write(dest_file);
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp} ({} files merged)",
        files.len()
    ));
    Ok(())
}

pub struct RuleOptions<'a> {
    pub extension: &'a str,
    pub header: &'a str,
    pub scoped_header: &'a str,
    pub include: &'a str,
    pub exclude: &'a str,
}

/// `sync_rules`: copy with the extension and header applied, then prune
/// managed files the source no longer has.
pub fn sync_rules(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    opts: &RuleOptions<'_>,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        s.log.warning(&format!("Rules source not found: {src_dir}"));
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_dir);
    s.ws.create_dir_all(dest_dir);

    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !filters::matches(&name, opts.include, opts.exclude) {
            continue;
        }
        let dest_name = if opts.extension.is_empty() {
            name.clone()
        } else {
            format!(
                "{}{}",
                name.strip_suffix(".md").unwrap_or(&name),
                opts.extension
            )
        };
        let dest_path = format!("{dest_dir}/{dest_name}");
        let bytes = read(s, &format!("{src_dir}/{name}"))?;
        let rendered = apply_rule_header(&bytes, opts.header, opts.scoped_header);
        s.ws.write(&dest_path, rendered)?;
        s.record_write(&dest_path);
        valid.push(dest_name);
    }

    let managed_suffix = if opts.extension.is_empty() {
        ".md"
    } else {
        opts.extension
    };
    let mut cleaned = 0usize;
    for name in s.ws.glob(dest_dir) {
        let path = format!("{dest_dir}/{name}");
        if !s.ws.is_file(&path) || !name.ends_with(managed_suffix) || valid.contains(&name) {
            continue;
        }
        if s.was_touched(&path) {
            continue;
        }
        s.ws.remove(&path);
        s.log.step(&format!("Removed: {dest_disp}/{name}"));
        cleaned += 1;
    }

    let extra = if opts.include.is_empty() {
        String::new()
    } else {
        format!(", include='{}'", opts.include)
    };
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ ({} updates, {cleaned} cleanups){extra}",
        valid.len()
    ));
    Ok(())
}

/// `inline_commands_to_file`.
pub fn inline_commands_to_file(
    s: &mut Session,
    src_dir: &str,
    target_file: &str,
    include: &str,
    exclude: &str,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) || target_file.is_empty() {
        return Ok(());
    }
    let mut entries = Vec::new();
    for name in md_files(s, src_dir) {
        if !filters::matches(&name, include, exclude) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name);
        let desc = convert::read_field(&read(s, &format!("{src_dir}/{name}"))?, "description");
        entries.extend_from_slice(format!("- `/{stem}`").as_bytes());
        if !desc.is_empty() {
            entries.extend_from_slice(" — ".as_bytes());
            entries.extend(desc);
        }
        entries.push(b'\n');
    }
    if entries.is_empty() {
        return Ok(());
    }
    let mut block = "\n## Commands\n\nThe following commands provide quick workflows. Find them in `.ai/src/commands/`:\n\n"
        .as_bytes()
        .to_vec();
    block.extend(entries);
    s.ws.append(target_file, &block)?;
    s.record_write(target_file);
    s.log.step(&format!(
        "Appended command index to {}",
        paths::leaf(target_file)
    ));
    Ok(())
}

/// `sync_commands_as_skills`.
pub fn sync_commands_as_skills(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    include: &str,
    exclude: &str,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_dir);
    s.ws.create_dir_all(dest_dir);

    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !filters::matches(&name, include, exclude) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let skill_dir = format!("{dest_dir}/command-{stem}");
        valid.push(format!("command-{stem}"));
        let source = read(s, &format!("{src_dir}/{name}"))?;

        s.ws.create_dir_all(&skill_dir);
        let skill_file = format!("{skill_dir}/SKILL.md");
        s.ws.write(&skill_file, convert::command_to_skill(&stem, &source))?;
        s.record_write(&skill_file);

        let policy = format!("{skill_dir}/agents/openai.yaml");
        if convert::read_field(&source, "disable-model-invocation") == b"true" {
            s.ws.create_dir_all(&format!("{skill_dir}/agents"));
            s.ws.write(
                &policy,
                b"policy:\n  allow_implicit_invocation: false\n".to_vec(),
            )?;
            s.record_write(&policy);
        } else if s.ws.is_file(&policy) {
            s.ws.remove(&policy);
            let agents_dir = format!("{skill_dir}/agents");
            if s.ws.list(&agents_dir).is_empty() {
                s.ws.remove(&agents_dir);
            }
        }
    }

    for name in s.ws.glob(dest_dir) {
        if !name.starts_with("command-") || !s.ws.is_dir(&format!("{dest_dir}/{name}")) {
            continue;
        }
        if !valid.contains(&name) {
            s.ws.remove(&format!("{dest_dir}/{name}"));
            s.log
                .step(&format!("Removed obsolete generated skill: {name}"));
        }
    }
    s.log.step(&format!(
        "{src_disp}/*.md → {dest_disp}/command-*/SKILL.md ({} generated)",
        valid.len()
    ));
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Conversion {
    CommandToml,
    AgentToml,
    AgentAmazonqJson,
    AgentOpencodeMd,
}

impl Conversion {
    fn extension(self) -> &'static str {
        match self {
            Self::CommandToml | Self::AgentToml => ".toml",
            Self::AgentAmazonqJson => ".json",
            Self::AgentOpencodeMd => ".md",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CommandToml => "commands, md→toml",
            Self::AgentToml => "agents, md→toml",
            Self::AgentAmazonqJson => "agents, md→amazonq json",
            Self::AgentOpencodeMd => "agents, md→opencode md",
        }
    }

    fn render(self, stem: &str, source: &[u8]) -> Vec<u8> {
        match self {
            Self::CommandToml => convert::command_to_toml(source),
            Self::AgentToml => convert::agent_to_toml(stem, source),
            Self::AgentAmazonqJson => convert::agent_to_amazonq_json(stem, source),
            Self::AgentOpencodeMd => convert::agent_to_opencode_md(stem, source),
        }
    }
}

/// `sync_commands_as_toml`, `sync_agents_as_toml`, `sync_agents_as_amazonq_json`,
/// and `sync_agents_as_opencode_md`, with `_sweep_generated` after them.
pub fn sync_converted(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    conversion: Conversion,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        return Ok(());
    }
    let ext = conversion.extension();
    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let dest_name = format!("{stem}{ext}");
        let dest_file = format!("{dest_dir}/{dest_name}");
        let source = read(s, &format!("{src_dir}/{name}"))?;
        s.ws.create_dir_all(dest_dir);
        s.ws.write(&dest_file, conversion.render(&stem, &source))?;
        s.record_write(&dest_file);
        valid.push(dest_name);
    }

    if s.ws.is_dir(dest_dir) {
        let dest_disp = s.display(dest_dir);
        for name in s.ws.glob(dest_dir) {
            let path = format!("{dest_dir}/{name}");
            if !name.ends_with(ext) || !s.ws.is_file(&path) || valid.contains(&name) {
                continue;
            }
            if s.was_touched(&path) {
                continue;
            }
            s.ws.remove(&path);
            s.log.step(&format!("Removed: {dest_disp}/{name}"));
        }
    }

    if !valid.is_empty() {
        let src_disp = s.display(src_dir);
        let dest_disp = s.display(dest_dir);
        s.log.step(&format!(
            "{src_disp}/ → {dest_disp}/ ({} {})",
            valid.len(),
            conversion.label()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    const CURSOR_HEADER: &str = "---\\nglobs: '**/*'\\nalwaysApply: true\\n---";
    const CURSOR_SCOPED: &str = "---\\nglobs: '{globs}'\\nalwaysApply: false\\n---";

    #[test]
    fn a_plain_rule_gets_the_expanded_header_and_a_blank_line() {
        assert_eq!(
            String::from_utf8(apply_rule_header(b"# Core\n", CURSOR_HEADER, CURSOR_SCOPED))
                .unwrap(),
            "---\nglobs: '**/*'\nalwaysApply: true\n---\n\n# Core\n"
        );
    }

    #[test]
    fn a_rule_with_frontmatter_keeps_its_keys_and_gains_missing_ones() {
        let merged = merge_or_prepend_header(b"---\nglobs: src/**\n---\n# R", CURSOR_HEADER);
        assert_eq!(
            String::from_utf8(merged).unwrap(),
            "---\nglobs: src/**\nalwaysApply: true\n---\n# R\n"
        );
    }

    // Design spec, "Known quirks", item 10: reproduced on purpose until cutover.
    #[test]
    fn a_paths_scoped_rule_takes_every_list_item_like_bash_does() {
        let rule = b"---\npaths:\n  - \"a/*\"\n  - b\ntags:\n  - 'c\n---\n\n\n# T\nbody";
        assert_eq!(rule_paths_csv(rule), b"a/*,b,c");
        assert_eq!(
            String::from_utf8(apply_rule_header(rule, CURSOR_HEADER, CURSOR_SCOPED)).unwrap(),
            "---\nglobs: 'a/*,b,c'\nalwaysApply: false\n---\n\n# T\nbody"
        );
        assert_eq!(rule_paths_csv(b"---\ntags:\n  - x\n---\n"), b"");
    }

    #[test]
    fn claude_rules_without_headers_are_copied_verbatim() {
        let rule = b"---\npaths:\n  - x\n---\nbody\n";
        assert_eq!(apply_rule_header(rule, "", ""), rule);
    }

    #[test]
    fn sync_rules_renames_and_prunes_obsolete_managed_files() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(&mut s, "/proj/.cursor/rules/old.mdc", "x");
        file(&mut s, "/proj/.cursor/rules/notes.txt", "keep");
        let opts = RuleOptions {
            extension: ".mdc",
            header: CURSOR_HEADER,
            scoped_header: CURSOR_SCOPED,
            include: "",
            exclude: "",
        };
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        assert!(text_of(&s, "/proj/.cursor/rules/core.mdc").starts_with("---\nglobs: '**/*'"));
        assert!(!s.ws.exists("/proj/.cursor/rules/old.mdc"));
        assert!(s.ws.exists("/proj/.cursor/rules/notes.txt"));
        assert_eq!(
            s.log.tail(1),
            ["   📁 .ai/src/rules/ → .cursor/rules/ (1 updates, 1 cleanups)"]
        );
    }

    #[test]
    fn merged_rules_are_separated_and_prefixed_by_agents() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agents\n");
        file(&mut s, "/proj/.ai/src/rules/a.md", "A\n");
        file(&mut s, "/proj/.ai/src/rules/b.md", "B\n");
        merge_rules_to_file(
            &mut s,
            "/proj/.ai/src/rules",
            "/proj/.rules",
            "",
            "",
            Some("/proj/.ai/src/AGENTS.md"),
        )
        .unwrap();
        assert_eq!(
            text_of(&s, "/proj/.rules"),
            "# Agents\n\n---\n\nA\n\n---\n\nB\n"
        );
    }

    #[test]
    fn imports_list_every_markdown_rule_in_the_dest() {
        let mut s = test_session();
        file(&mut s, "/proj/CLAUDE.md", "# A\n");
        file(&mut s, "/proj/.claude/rules/core.md", "");
        file(&mut s, "/proj/.claude/rules/git.md", "");
        append_imports(&mut s, "/proj/CLAUDE.md", "/proj/.claude/rules").unwrap();
        assert_eq!(
            text_of(&s, "/proj/CLAUDE.md"),
            "# A\n\n<!-- Auto-generated imports -->\n@rules/core.md\n@rules/git.md\n"
        );
    }

    #[test]
    fn the_command_index_uses_descriptions_when_present() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: Review\n---\n",
        );
        file(&mut s, "/proj/.ai/src/commands/ship.md", "Ship\n");
        file(&mut s, "/proj/AGENTS.md", "# A\n");
        inline_commands_to_file(&mut s, "/proj/.ai/src/commands", "/proj/AGENTS.md", "", "")
            .unwrap();
        assert_eq!(
            text_of(&s, "/proj/AGENTS.md"),
            "# A\n\n## Commands\n\nThe following commands provide quick workflows. Find them in `.ai/src/commands/`:\n\n- `/review` — Review\n- `/ship`\n"
        );
    }

    #[test]
    fn command_skills_carry_the_codex_opt_out_and_drop_stale_dirs() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/commands/only.md",
            "---\ndescription: D\ndisable-model-invocation: true\n---\nBody\n",
        );
        file(&mut s, "/proj/.agents/skills/command-gone/SKILL.md", "x");
        file(&mut s, "/proj/.agents/skills/mine/SKILL.md", "x");
        sync_commands_as_skills(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.agents/skills",
            "",
            "",
        )
        .unwrap();
        assert_eq!(
            text_of(&s, "/proj/.agents/skills/command-only/agents/openai.yaml"),
            "policy:\n  allow_implicit_invocation: false\n"
        );
        assert!(!s.ws.exists("/proj/.agents/skills/command-gone"));
        assert!(s.ws.exists("/proj/.agents/skills/mine"));
    }

    #[test]
    fn converted_directories_sweep_their_own_extension_only() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/agents/rev.md",
            "---\nname: rev\n---\nBody\n",
        );
        file(&mut s, "/proj/.codex/agents/old.toml", "x");
        file(&mut s, "/proj/.codex/agents/keep.json", "x");
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
        )
        .unwrap();
        assert!(s.ws.is_file("/proj/.codex/agents/rev.toml"));
        assert!(!s.ws.exists("/proj/.codex/agents/old.toml"));
        assert!(s.ws.exists("/proj/.codex/agents/keep.json"));
        assert_eq!(
            s.log.tail(1),
            ["   📁 .ai/src/agents/ → .codex/agents/ (1 agents, md→toml)"]
        );
    }
}
```

- [x] **Step 4: Register the modules in `src/lib.rs`**

```rust
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
pub mod paths;
pub mod payload;
pub mod project;
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
```

- [x] **Step 5: Record the quirk in the design spec**

After item 9 under "Known quirks", add:

```markdown
10. `_rule_paths_csv` collects every list item in a rule's frontmatter once it
    has a bare `paths:` key, not only the items under `paths:`.
```

- [x] **Step 6: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `89 passed` (73 + 2 `session` + 4 `file_ops` + 10 `rules`).

- [x] **Step 7: Cross-check a merged header and a scoped rule against Bash**

```bash
dir=$(mktemp -d "${TMPDIR:-/tmp}/rules.XXXXXX")
printf -- '---\nglobs: src/**\n---\n# R' > "$dir/fm.md"
printf -- '---\npaths:\n  - "a/*"\n  - b\ntags:\n  - '"'"'c\n---\n\n\n# T\nbody' > "$dir/scoped.md"
bash -c 'source lib/helpers/logging.sh; source lib/helpers/filters.sh; source lib/helpers/file_ops.sh; source lib/helpers/format_conversion.sh; source lib/helpers/rule_operations.sh
  H="---\\nglobs: '"'"'**/*'"'"'\\nalwaysApply: true\\n---"; S="---\\nglobs: '"'"'{globs}'"'"'\\nalwaysApply: false\\n---"
  merge_or_prepend_header "$1/fm.md" "$H"; printf "%q\n" "$(cat "$1/fm.md"; echo x)"
  apply_rule_header "$1/scoped.md" "$H" "$S"; printf "%q\n" "$(cat "$1/scoped.md"; echo x)"' _ "$dir"
```

Expected, the bytes the `rules` tests assert:

```text
$'---\nglobs: src/**\nalwaysApply: true\n---\n# R\nx'
$'---\nglobs: \'a/*,b,c\'\nalwaysApply: false\n---\n\n# T\nbodyx'
```

- [x] **Step 8: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/session.rs src/file_ops.rs src/rules.rs src/lib.rs docs/specs/2026-09-12-rust-migration-design.md
git commit -m "feat(native): port file operations and rule directory sync"
```

---

### Task 7: Tool Flags and Filters, Payload Resolution, Profiles, Overlays

**Files:**
- Modify: `src/tool.rs` (add `new`, `user_value`, `flag`, `filter`)
- Modify: `src/payload.rs` (add `resolve_source`, `describe_source`)
- Create: `src/profiles.rs`
- Create: `src/overlay.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Session` (Task 6), `Workspace` (Task 3), `paths` (Task 2), `catalog`, `yaml_subset`.
- Produces:
  - `Tool::new(slug, user_yaml: Option<String>) -> Tool`, `Tool::user_value(key) -> String`, `Tool::flag(key) -> Option<bool>` (`get_tool_bool`), `Tool::filter(key) -> String` (`get_tool_filter`). `Tool::load`, `value`, `display_name`, `base_payload`, `base_name` are unchanged, so `list` is untouched.
  - `payload::resolve_source(&mut Session, &Tool, resource) -> Option<String>` (`resolve_payload_source`; base templates come back as `/<agentsync>/lib/templates/<resource>/<file>`), `payload::describe_source(root, path, slug, resource) -> &'static str`.
  - `profiles::names(config) -> Vec<String>`, `overlay_dir(config, name) -> String`, `tools(config, name) -> Vec<String>`, `is_active(config, name) -> bool`, `all_tools(config) -> Vec<String>`.
  - `overlay::Sources { agents, rules, skills, commands, subagents }` (`Clone`, `Default`, `PartialEq`); `build_tree(&mut Workspace, name, child_src, parent_src, &[&str]) -> Result<String, Error>`; `rewrite_sources(&Workspace, dir, &mut Sources)`; `setup_base_src(&mut Session, config: Option<&str>, &mut Sources) -> Result<(), Error>`; `setup_profile(&mut Session, config, name, base_src, &mut Sources) -> Result<bool, Error>`; `cleanup_profile(&mut Workspace)`; `shared_parent_src(config, root) -> Option<String>`; `inherit_categories(raw) -> Vec<&'static str>`; `merge_shared_parent(&mut Workspace, parent_src, &[&str])`.

- [x] **Step 1: Replace `src/tool.rs`**

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
        Ok(Self::from_parts(
            slug,
            user_yaml,
            catalog::base_tool_yaml(slug),
        ))
    }

    /// A tool whose override text was read by the caller, with its shipped base.
    pub fn new(slug: &str, user_yaml: Option<String>) -> Self {
        Self::from_parts(slug, user_yaml, catalog::base_tool_yaml(slug))
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

    /// A scalar from `.ai/src/tools/<slug>.yaml` alone, as the legacy
    /// `enabled: true` lookup reads it.
    pub fn user_value(&self, key_path: &str) -> String {
        self.user_yaml
            .as_deref()
            .map(|text| yaml_subset::value(text, key_path))
            .unwrap_or_default()
    }

    /// `get_tool_bool`:`Some` for the true and false spellings, `None` otherwise.
    pub fn flag(&self, key_path: &str) -> Option<bool> {
        match self.value(key_path).to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" | "on" => Some(true),
            "false" | "no" | "0" | "off" => Some(false),
            _ => None,
        }
    }

    /// `get_tool_filter`: an include/exclude list as one space-joined string,
    /// layered like `value` but per file, from a scalar, `[a, b]`, or a block list.
    pub fn filter(&self, key_path: &str) -> String {
        if let Some(user) = &self.user_yaml {
            let found = read_filter(user, key_path);
            if !found.is_empty() {
                return found;
            }
        }
        if let Some(base) = self.base_yaml {
            return read_filter(base, key_path);
        }
        match catalog::base_tool_yaml(&self.base_name()) {
            Some(text) => read_filter(text, key_path),
            None => String::new(),
        }
    }

    pub fn display_name(&self) -> String {
        let name = self.value("name");
        if name.is_empty() {
            self.slug.clone()
        } else {
            name
        }
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

/// `_read_filter_file`.
fn read_filter(text: &str, key_path: &str) -> String {
    let scalar = yaml_subset::value(text, key_path);
    if !scalar.is_empty() && !(scalar.starts_with('[') && scalar.ends_with(']')) {
        return scalar;
    }
    yaml_subset::list(text, key_path).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_with(user: &str) -> Tool {
        Tool::from_parts(
            "claude",
            Some(user.to_string()),
            catalog::base_tool_yaml("claude"),
        )
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
        let payload = tool
            .base_payload("settings")
            .expect("inherits claude's settings");
        assert!(payload.path().ends_with("claude.json"));
    }

    #[test]
    fn flags_accept_the_bash_spellings_case_insensitively() {
        let tool =
            claude_with("targets:\n  rules:\n    enabled: OFF\n  skills:\n    enabled: maybe\n");
        assert_eq!(tool.flag("targets.rules.enabled"), Some(false));
        assert_eq!(tool.flag("targets.skills.enabled"), None);
    }

    #[test]
    fn filters_join_scalar_inline_and_block_forms() {
        let block = claude_with("targets:\n  skills:\n    exclude:\n      - a\n      - \"b*\"\n");
        assert_eq!(block.filter("targets.skills.exclude"), "a b*");
        let inline = claude_with("targets:\n  skills:\n    exclude: [a, b]\n");
        assert_eq!(inline.filter("targets.skills.exclude"), "a b");
        let scalar = claude_with("targets:\n  skills:\n    exclude: \"a b\"\n");
        assert_eq!(scalar.filter("targets.skills.exclude"), "a b");
        assert_eq!(claude_with("").filter("targets.rules.include"), "");
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

- [x] **Step 2: Replace `src/payload.rs`**

```rust
//! Where a tool's settings, mcp, or hooks override lives, mirroring the lookups
//! in `lib/helpers/tool_resolver.sh` that `list` reports on.

use std::path::PathBuf;

use crate::paths::{self, ENGINE_ROOT};
use crate::session::Session;
use crate::{Error, project::Project, tool::Tool};

/// `.ai/src/tools/<slug>/<resource>.*`, first by name: the layout since 0.11.
pub fn find_new_override(
    project: &Project,
    slug: &str,
    resource: &str,
) -> Result<Option<PathBuf>, Error> {
    let dir = project.user_tools_dir().join(slug);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(Error::io(dir, e)),
    };
    let prefix = format!("{resource}.");
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let is_match = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix));
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
    let ext = tool
        .base_payload(resource)?
        .path()
        .extension()?
        .to_str()?
        .to_string();
    Some(
        project
            .root
            .join(".ai")
            .join("src")
            .join(resource)
            .join(format!("{}.{ext}", tool.slug)),
    )
}

/// `resolve_payload_source`: per-tool override → declared `targets.<res>.source`
/// → legacy flat layout → shared `.ai/src/mcp.json` (mcp only) → shipped base.
pub fn resolve_source(s: &mut Session, tool: &Tool, resource: &str) -> Option<String> {
    let root = s.paths.root.clone();
    let override_dir = format!("{root}/.ai/src/tools/{}", tool.slug);
    if s.ws.is_dir(&override_dir) {
        let prefix = format!("{resource}.");
        let found =
            s.ws.glob(&override_dir)
                .into_iter()
                .map(|name| format!("{override_dir}/{name}"))
                .find(|path| paths::leaf(path).starts_with(&prefix) && s.ws.is_file(path));
        if found.is_some() {
            return found;
        }
    }

    let declared = tool.value(&format!("targets.{resource}.source"));
    if !declared.is_empty() {
        let declared_abs = if declared.starts_with('/') {
            declared.clone()
        } else {
            format!("{root}/{declared}")
        };
        if s.ws.is_file(&declared_abs) {
            if ["hooks", "mcp", "settings"]
                .iter()
                .any(|kind| declared.starts_with(&format!(".ai/src/{kind}/")))
            {
                s.warn_legacy_payload(&declared_abs);
            }
            return Some(declared_abs);
        }
    }

    let base = tool.base_payload(resource);
    if let Some(ext) = base
        .and_then(|file| file.path().extension())
        .and_then(|ext| ext.to_str())
    {
        let legacy = format!("{root}/.ai/src/{resource}/{}.{ext}", tool.slug);
        if s.ws.is_file(&legacy) {
            s.warn_legacy_payload(&legacy);
            return Some(legacy);
        }
    }

    if resource == "mcp" {
        let shared = format!("{root}/.ai/src/mcp.json");
        if s.ws.is_file(&shared) {
            return Some(shared);
        }
    }

    base.and_then(|file| file.path().file_name())
        .and_then(|name| name.to_str())
        .map(|name| format!("{ENGINE_ROOT}/lib/templates/{resource}/{name}"))
}

/// `describe_payload_source`.
pub fn describe_source(root: &str, path: &str, slug: &str, resource: &str) -> &'static str {
    if path.is_empty() {
        return "";
    }
    if path.starts_with(&format!("{root}/.ai/src/tools/{slug}/")) {
        return "override";
    }
    if resource == "mcp" && path == format!("{root}/.ai/src/mcp.json") {
        return "shared";
    }
    if path.starts_with(&format!("{root}/.ai/src/{resource}/")) {
        return "legacy";
    }
    if path.starts_with(&format!("{ENGINE_ROOT}/lib/templates/{resource}/")) {
        return "base";
    }
    "declared"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    #[test]
    fn payload_resolution_follows_the_bash_order() {
        let mut s = test_session();
        let claude = Tool::new("claude", None);
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/<agentsync>/lib/templates/mcp/claude.json")
        );
        s.ws.insert_file("/proj/.ai/src/mcp.json", Content::Bytes(b"{}".to_vec()));
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp.json")
        );
        s.ws.insert_file(
            "/proj/.ai/src/mcp/claude.json",
            Content::Bytes(b"{}".to_vec()),
        );
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp/claude.json")
        );
        assert_eq!(s.log.lines().len(), 3);
        s.ws.insert_file(
            "/proj/.ai/src/tools/claude/mcp.json",
            Content::Bytes(b"{}".to_vec()),
        );
        let found = resolve_source(&mut s, &claude, "mcp").unwrap();
        assert_eq!(found, "/proj/.ai/src/tools/claude/mcp.json");
        assert_eq!(
            describe_source("/proj", &found, "claude", "mcp"),
            "override"
        );
        assert_eq!(
            describe_source(
                "/proj",
                "/<agentsync>/lib/templates/mcp/claude.json",
                "claude",
                "mcp"
            ),
            "base"
        );
        assert_eq!(resolve_source(&mut s, &claude, "hooks"), None);
    }

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
        assert_eq!(
            found,
            Some(dir.path().join(".ai/src/tools/cursor/hooks.json"))
        );
        assert_eq!(
            find_new_override(&project, "cursor", "settings").unwrap(),
            None
        );
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

- [x] **Step 3: Write `src/profiles.rs`**

```rust
//! `profiles:` in `agent_sync.yaml`, read as `lib/helpers/profiles.sh` reads it.

use crate::yaml_subset;

/// `_profiles_names`: the child keys of the root `profiles:` mapping.
pub fn names(config: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_section = false;
    let mut child_indent: Option<usize> = None;
    for line in config.lines() {
        if line.is_empty() {
            continue;
        }
        let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if stripped.starts_with('#') {
            continue;
        }
        let indent = line.len() - stripped.len();
        let key = stripped.split_once(':').and_then(|(key, _)| {
            (!key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .then_some(key)
        });
        if !in_section {
            if indent == 0 && key == Some("profiles") {
                in_section = true;
            }
            continue;
        }
        if indent == 0 {
            break;
        }
        let Some(key) = key else {
            continue;
        };
        let expected = *child_indent.get_or_insert(indent);
        if indent == expected {
            names.push(key.to_string());
        }
    }
    names
}

/// `profile_overlay_dir`: `profiles.<name>.overlay`, else `.ai/profiles/<name>`.
pub fn overlay_dir(config: &str, name: &str) -> String {
    let value = yaml_subset::value(config, &format!("profiles.{name}.overlay"));
    if value.is_empty() {
        format!(".ai/profiles/{name}")
    } else {
        value
    }
}

/// `profile_tools`.
pub fn tools(config: &str, name: &str) -> Vec<String> {
    yaml_subset::list(config, &format!("profiles.{name}.tools"))
}

/// `profile_is_active`.
pub fn is_active(config: &str, name: &str) -> bool {
    matches!(
        yaml_subset::value(config, &format!("profiles.{name}.active"))
            .to_ascii_lowercase()
            .as_str(),
        "true" | "yes" | "1" | "on"
    )
}

/// `list_profile_tools`: every profile's tools, sorted and deduplicated.
pub fn all_tools(config: &str) -> Vec<String> {
    let mut all: Vec<String> = names(config)
        .iter()
        .flat_map(|name| tools(config, name))
        .collect();
    all.sort();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "tools:\n  enabled: [claude]\nprofiles:\n  # personal\n  hub:\n    overlay: \".ai/profiles/hub\"\n    active: true\n    tools: [claude-hub, codex-hub]\n  work:\n    active: no\n    tools:\n      - claude-work\n      - claude-hub\noutputs: local\n";

    #[test]
    fn profile_names_are_the_direct_children_of_profiles() {
        assert_eq!(names(CONFIG), ["hub", "work"]);
        assert!(names("tools:\n  enabled: []\n").is_empty());
    }

    #[test]
    fn profile_fields_read_with_defaults() {
        assert!(is_active(CONFIG, "hub"));
        assert!(!is_active(CONFIG, "work"));
        assert_eq!(overlay_dir(CONFIG, "work"), ".ai/profiles/work");
        assert_eq!(tools(CONFIG, "work"), ["claude-work", "claude-hub"]);
        assert_eq!(
            all_tools(CONFIG),
            ["claude-hub", "claude-work", "codex-hub"]
        );
    }
}
```

- [x] **Step 4: Write `src/overlay.rs`**

```rust
//! Source overlays of `lib/helpers/shared.sh`: the engine-owned skill layer,
//! per-profile overlays, and the `shared:` parent files `lib/check.sh` merges
//! into its workspace. Overlay trees live under the virtual overlay root.

use std::path::{Path, PathBuf};

use crate::paths::{self, ENGINE_ROOT, OVERLAY_ROOT};
use crate::session::Session;
use crate::workspace::{Content, Workspace};
use crate::{Error, profiles, yaml_subset};

const CATEGORIES: [&str; 4] = ["rules", "skills", "commands", "agents"];

/// The `SOURCE_*` paths a render reads from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub agents: String,
    pub rules: String,
    pub skills: String,
    pub commands: String,
    pub subagents: String,
}

/// `build_overlay_tree`: mirror the child's `AGENTS.md` and category trees,
/// then fill each category with parent files the child lacks. Returns the
/// overlay directory; its `src/` holds the tree.
pub fn build_tree(
    ws: &mut Workspace,
    name: &str,
    child_src: &str,
    parent_src: &str,
    categories: &[&str],
) -> Result<String, Error> {
    let dir = format!("{OVERLAY_ROOT}/{name}");
    ws.remove(&dir);
    let src = format!("{dir}/src");
    ws.create_dir_all(&src);

    if ws.is_dir(child_src) {
        let agents = format!("{child_src}/AGENTS.md");
        if ws.is_file(&agents) {
            ws.copy(&agents, &format!("{src}/AGENTS.md"))?;
        }
        for item in CATEGORIES {
            let from = format!("{child_src}/{item}");
            if ws.is_dir(&from) {
                ws.copy(&from, &format!("{src}/{item}"))?;
            }
        }
    }

    for category in categories {
        let parent_dir = format!("{parent_src}/{category}");
        if !ws.is_dir(&parent_dir) {
            continue;
        }
        for file in ws.files_under(&parent_dir) {
            let rel = &file[parent_dir.len() + 1..];
            let target = format!("{src}/{category}/{rel}");
            if ws.exists(&target) {
                continue;
            }
            ws.create_dir_all(&paths::parent(&target));
            ws.copy(&file, &target)?;
        }
    }
    Ok(dir)
}

/// `_overlay_rewrite_sources`: only the paths the overlay materialised.
pub fn rewrite_sources(ws: &Workspace, dir: &str, sources: &mut Sources) {
    let src = format!("{dir}/src");
    if ws.is_file(&format!("{src}/AGENTS.md")) {
        sources.agents = format!("{src}/AGENTS.md");
    }
    for (category, slot) in [
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("agents", &mut sources.subagents),
    ] {
        let path = format!("{src}/{category}");
        if ws.is_dir(&path) {
            *slot = path;
        }
    }
}

/// `base_src_setup_overlay`: engine-owned skills fill paths the project lacks,
/// unless `base_skills: false`.
pub fn setup_base_src(
    s: &mut Session,
    config: Option<&str>,
    sources: &mut Sources,
) -> Result<(), Error> {
    let base_src = format!("{ENGINE_ROOT}/lib/templates/base-src");
    if !s.ws.is_dir(&format!("{base_src}/skills")) {
        return Ok(());
    }
    if config.is_some_and(|text| yaml_subset::value(text, "base_skills") == "false") {
        return Ok(());
    }
    let child_src = format!("{}/.ai/src", s.paths.root);
    if !s.ws.is_dir(&child_src) {
        return Ok(());
    }
    let dir = build_tree(&mut s.ws, "base-src", &child_src, &base_src, &["skills"])?;
    rewrite_sources(&s.ws, &dir, sources);
    Ok(())
}

/// `profile_setup_overlay`: false when the profile has no `src/` of its own.
pub fn setup_profile(
    s: &mut Session,
    config: &str,
    name: &str,
    base_src: &str,
    sources: &mut Sources,
) -> Result<bool, Error> {
    let overlay = profiles::overlay_dir(config, name);
    let overlay_root = if overlay.starts_with('/') {
        overlay
    } else {
        format!("{}/{overlay}", s.paths.root)
    };
    let profile_src = format!("{overlay_root}/src");
    if !s.ws.is_dir(&profile_src) {
        return Ok(false);
    }
    let dir = build_tree(&mut s.ws, "profile", &profile_src, base_src, &CATEGORIES)?;
    rewrite_sources(&s.ws, &dir, sources);
    s.log
        .info(&format!("Profile overlay active: {name} ({profile_src})"));
    Ok(true)
}

pub fn cleanup_profile(ws: &mut Workspace) {
    ws.remove(&format!("{OVERLAY_ROOT}/profile"));
}

/// `shared_parent_src`: the parent's `.ai/src/`, resolved on disk from the root.
pub fn shared_parent_src(config: &str, root: &str) -> Option<String> {
    let raw = yaml_subset::value(config, "shared.path");
    if raw.is_empty() {
        return None;
    }
    let parent_root = if raw.starts_with('/') {
        raw
    } else {
        format!("{root}/{raw}")
    };
    if !Path::new(&parent_root).is_dir() {
        return None;
    }
    let parent_root = paths::normalize(&parent_root);
    let nested = format!("{parent_root}/.ai/src");
    let parent_src = if Path::new(&nested).is_dir() {
        nested
    } else if paths::leaf(&parent_root) == "src" {
        parent_root
    } else {
        return None;
    };
    (parent_src != format!("{root}/.ai/src")).then_some(parent_src)
}

/// `shared_inherit_categories`: the inherit tokens sync materialises.
pub fn inherit_categories(raw: &str) -> Vec<&'static str> {
    raw.split([',', ' ', '\t', '\n'])
        .filter_map(|token| match token {
            "subagents" | "agents" => Some("agents"),
            "rules" => Some("rules"),
            "skills" => Some("skills"),
            "commands" => Some("commands"),
            _ => None,
        })
        .collect()
}

/// The `shared:` block of `lib/check.sh`: parent files of the inherited
/// categories are merged into the workspace's `.ai/src/` where the project
/// has no file at that path, read from disk as `find -type f` lists them.
pub fn merge_shared_parent(ws: &mut Workspace, parent_src: &str, categories: &[&str]) {
    let child_src = format!("{}/.ai/src", ws.root());
    ws.create_dir_all(&child_src);
    for category in categories {
        let parent_dir = PathBuf::from(format!("{parent_src}/{category}"));
        if !parent_dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_regular_files(&parent_dir, "", &mut files);
        for (rel, disk) in files {
            let target = format!("{child_src}/{category}/{rel}");
            if !ws.exists(&target) {
                ws.insert_file(&target, Content::Disk(disk));
            }
        }
    }
}

fn collect_regular_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            collect_regular_files(&entry.path(), &child_rel, out);
        } else if meta.is_file() {
            out.push((child_rel, entry.path()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;

    fn file(ws: &mut Workspace, path: &str, text: &str) {
        ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    #[test]
    fn the_child_wins_and_the_parent_fills_only_listed_categories() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "child");
        file(&mut s.ws, "/proj/.ai/src/skills/a/SKILL.md", "child a");
        file(&mut s.ws, "/proj/parent/skills/a/SKILL.md", "parent a");
        file(&mut s.ws, "/proj/parent/skills/a/extra.md", "parent extra");
        file(&mut s.ws, "/proj/parent/rules/p.md", "parent rule");
        let dir = build_tree(&mut s.ws, "t", "/proj/.ai/src", "/proj/parent", &["skills"]).unwrap();
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/SKILL.md")).unwrap(),
            b"child a"
        );
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/extra.md")).unwrap(),
            b"parent extra"
        );
        assert!(!s.ws.exists(&format!("{dir}/src/rules")));

        let mut sources = Sources {
            rules: "/proj/.ai/src/rules".into(),
            ..Sources::default()
        };
        rewrite_sources(&s.ws, &dir, &mut sources);
        assert_eq!(sources.agents, "/<agentsync-overlay>/t/src/AGENTS.md");
        assert_eq!(sources.skills, "/<agentsync-overlay>/t/src/skills");
        assert_eq!(sources.rules, "/proj/.ai/src/rules");
    }

    #[test]
    fn the_base_skill_layer_is_skipped_by_base_skills_false() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "a");
        let mut sources = Sources::default();
        setup_base_src(&mut s, Some("base_skills: false\n"), &mut sources).unwrap();
        assert_eq!(sources, Sources::default());
        setup_base_src(&mut s, None, &mut sources).unwrap();
        assert_eq!(sources.skills, "/<agentsync-overlay>/base-src/src/skills");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/agentsync/SKILL.md"));
    }

    #[test]
    fn a_profile_without_src_leaves_sources_alone() {
        let mut s = test_session();
        let config = "profiles:\n  hub:\n    tools: [claude-hub]\n";
        let mut sources = Sources::default();
        assert!(!setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        file(&mut s.ws, "/proj/.ai/profiles/hub/src/rules/hub.md", "h");
        assert!(setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        assert_eq!(sources.rules, "/<agentsync-overlay>/profile/src/rules");
        assert_eq!(
            s.log.tail(1),
            ["[INFO] Profile overlay active: hub (/proj/.ai/profiles/hub/src)"]
        );
    }

    #[test]
    fn inherit_tokens_are_validated_like_shared_setup_overlay() {
        assert_eq!(
            inherit_categories("rules, tools,subagents"),
            ["rules", "agents"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_parent_resolves_on_disk_and_never_to_the_project_itself() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("child");
        std::fs::create_dir_all(root.join(".ai/src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".ai/src/rules")).unwrap();
        std::fs::write(dir.path().join(".ai/src/rules/p.md"), "p").unwrap();
        let root = root.to_string_lossy().into_owned();
        let parent = shared_parent_src("shared:\n  path: \"../\"\n", &root).unwrap();
        assert_eq!(parent, format!("{}/.ai/src", dir.path().to_string_lossy()));
        assert_eq!(shared_parent_src("shared:\n  path: \".\"\n", &root), None);

        let mut ws = Workspace::new(&root);
        merge_shared_parent(&mut ws, &parent, &["rules"]);
        assert_eq!(
            ws.read(&format!("{root}/.ai/src/rules/p.md")).unwrap(),
            b"p"
        );
    }
}
```

- [x] **Step 5: Register the modules in `src/lib.rs`**

```rust
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
```

- [x] **Step 6: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `99 passed` (89 + 2 `tool` + 1 `payload` + 2 `profiles` + 5 `overlay`).

- [x] **Step 7: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/tool.rs src/payload.rs src/profiles.rs src/overlay.rs src/lib.rs
git commit -m "feat(native): resolve payloads, profiles, and source overlays"
```

---

### Task 8: The Render

**Files:**
- Create: `src/render.rs`
- Modify: `src/lib.rs`
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` ("Known quirks", item 11)

**Interfaces:**
- Consumes: everything from Tasks 1–7.
- Produces: `render::render(&mut Session, &Env) -> Result<(), Stop>`; `render::Env { pub config_path: Option<String> }` (`Default`; `AGENTSYNC_CONFIG_PATH`); `render::Stop(pub u8)` — the status `sync.sh` would exit with, after its log lines. On success the session's workspace holds every output and `touched()` the manifest's new paths. The render is `sync --force` with `AGENTSYNC_SKIP_POST_SYNC=true` and no backup: the run `lib/check.sh` started.

- [x] **Step 1: Write `src/render.rs`**

```rust
//! What `lib/sync.sh --force` computes, without the transaction: config and
//! sources, the engine skill layer, every enabled tool and active profile
//! rendered into the workspace, disabled tools cleaned. Phase 3 adds the
//! manifest, backup, `.gitignore`, dry-run, and filter options around it.

use std::collections::BTreeSet;

use crate::overlay::{self, Sources};
use crate::rules::{self, Conversion, RuleOptions};
use crate::session::Session;
use crate::tool::Tool;
use crate::{
    Error, catalog, engine_version, file_ops, opencode_json, paths, payload, profiles, yaml_subset,
};

const TARGET_KEYS: [&str; 9] = [
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

type Step = Result<(), Stop>;

/// Environment the render reads; `check` passes the process environment.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
}

struct Run {
    config: Option<String>,
    cleanup: String,
    sources: Sources,
    enabled: BTreeSet<String>,
    profile_tools: BTreeSet<String>,
    protected: Vec<String>,
    tools: Vec<String>,
    printed: bool,
}

#[derive(Default)]
struct Dests {
    agents: String,
    rules: String,
    skills: String,
    commands: String,
    subagents: String,
    settings: String,
    mcp: String,
    hooks: String,
    guard: String,
}

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

pub fn render(s: &mut Session, env: &Env) -> Step {
    let mut run = load_run_config(s, env)?;
    resolve_sources(s, &mut run)?;
    check_version_pin(s, &run)?;

    s.log.separator();
    s.log.info("Starting AgentSync Config Sync...");
    s.log.separator();
    s.log.out(String::new());

    let config = run.config.clone();
    overlay::setup_base_src(s, config.as_deref(), &mut run.sources).map_err(|e| io(s, e))?;
    let base_sources = run.sources.clone();
    let profile_base_src = format!("{}/.ai/src", s.paths.root);

    let active_profiles: Vec<String> = config
        .as_deref()
        .map(|text| {
            profiles::names(text)
                .into_iter()
                .filter(|name| profiles::is_active(text, name))
                .collect()
        })
        .unwrap_or_default();
    load_tools(s, &mut run);
    collect_protected_dests(s, &mut run);

    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        if run.enabled.contains(slug) {
            sync_tool(s, &mut run, slug)?;
        } else {
            cleanup_tool(s, &mut run, slug);
        }
        if run.printed {
            s.log.out(String::new());
        }
    }

    for profile in active_profiles {
        let text = config.clone().unwrap_or_default();
        let tools: Vec<String> = profiles::tools(&text, &profile)
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        if tools.is_empty() {
            continue;
        }
        s.log.separator();
        s.log.info(&format!("Profile: {profile}"));
        run.sources = base_sources.clone();
        overlay::setup_profile(s, &text, &profile, &profile_base_src, &mut run.sources)
            .map_err(|e| io(s, e))?;
        for slug in tools {
            sync_tool(s, &mut run, &slug)?;
            if run.printed {
                s.log.out(String::new());
            }
        }
        overlay::cleanup_profile(&mut s.ws);
    }
    Ok(())
}

/// `resolve_project_config_path` and `_load_run_config`.
fn load_run_config(s: &mut Session, env: &Env) -> Result<Run, Stop> {
    let root = s.paths.root.clone();
    let mut config_path = None;
    if let Some(raw) = env.config_path.as_deref().filter(|p| !p.is_empty()) {
        let path = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("{root}/{raw}")
        };
        if s.ws.is_file(&path) {
            config_path = Some(path);
        } else {
            s.log.warning(&format!(
                "AGENTSYNC_CONFIG_PATH is set but file not found: {path}"
            ));
        }
    }
    if config_path.is_none() {
        config_path = [
            format!("{root}/.ai/agent_sync.yaml"),
            format!("{root}/agent_sync.yaml"),
        ]
        .into_iter()
        .find(|path| s.ws.is_file(path));
    }
    let config = match &config_path {
        Some(path) => {
            let bytes = s.ws.read(path).map_err(|e| io(s, e))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        None => None,
    };

    let mut cleanup = "true".to_string();
    if let (Some(text), Some(path)) = (&config, &config_path) {
        let configured = yaml_subset::value(text, "defaults.cleanup");
        if !configured.is_empty() {
            cleanup = configured;
        }
        let outputs = yaml_subset::value(text, "outputs").replace('"', "");
        if !matches!(outputs.as_str(), "" | "committed" | "local") {
            let shown = path
                .strip_prefix(&format!("{root}/"))
                .unwrap_or(path)
                .to_string();
            s.log.error(&format!(
                "Unknown outputs mode '{outputs}' in {shown} — expected 'committed' or 'local'"
            ));
            return Err(Stop(1));
        }
    }

    Ok(Run {
        config,
        cleanup,
        sources: Sources::default(),
        enabled: BTreeSet::new(),
        profile_tools: BTreeSet::new(),
        protected: Vec::new(),
        tools: Vec::new(),
        printed: false,
    })
}

fn outputs_mode(config: &str) -> &'static str {
    match yaml_subset::value(config, "outputs")
        .replace('"', "")
        .as_str()
    {
        "committed" => "committed",
        "local" => "local",
        _ if yaml_subset::value(config, "gitignore.update") == "false" => "committed",
        _ => "local",
    }
}

/// `_resolve_sources`.
fn resolve_sources(s: &mut Session, run: &mut Run) -> Step {
    let global = catalog::GLOBAL_CONFIG;
    let root = s.paths.root.clone();
    let detect = |s: &Session, is_file: bool, sub: &str| -> Option<String> {
        [format!(".ai/src/{sub}"), format!(".ai/{sub}")]
            .into_iter()
            .find(|rel| {
                let abs = format!("{root}/{rel}");
                if is_file {
                    s.ws.is_file(&abs)
                } else {
                    s.ws.is_dir(&abs)
                }
            })
    };
    let mut sources = Sources {
        agents: yaml_subset::value(global, "source.agents"),
        rules: yaml_subset::value(global, "source.rules"),
        skills: yaml_subset::value(global, "source.skills"),
        commands: String::new(),
        subagents: String::new(),
    };
    for (is_file, sub, slot) in [
        (true, "AGENTS.md", &mut sources.agents),
        (false, "rules", &mut sources.rules),
        (false, "skills", &mut sources.skills),
        (false, "commands", &mut sources.commands),
        (false, "agents", &mut sources.subagents),
    ] {
        if let Some(found) = detect(s, is_file, sub) {
            *slot = found;
        }
    }
    if let Some(text) = &run.config {
        for (key, slot) in [
            ("agents", &mut sources.agents),
            ("rules", &mut sources.rules),
            ("skills", &mut sources.skills),
            ("commands", &mut sources.commands),
            ("subagents", &mut sources.subagents),
        ] {
            let nested = yaml_subset::value(text, &format!("source.{key}"));
            let chosen = if nested.is_empty() {
                yaml_subset::value(text, key)
            } else {
                nested
            };
            if !chosen.is_empty() {
                *slot = chosen;
            }
        }
    }

    let agents_abs = s
        .paths
        .clone()
        .resolve_source(&sources.agents, "source.agents", &mut s.log)
        .ok_or(Stop(1))?;
    if !s.ws.is_file(&agents_abs) {
        s.log
            .error(&format!("Source agents file not found: {agents_abs}"));
        s.log
            .error("Run 'agentsync init' or set source.agents in agent_sync.yaml");
        return Err(Stop(1));
    }
    run.sources = sources;
    Ok(())
}

/// `_check_version_pin_or_exit`.
fn check_version_pin(s: &mut Session, run: &Run) -> Step {
    let Some(config) = &run.config else {
        return Ok(());
    };
    let pinned = yaml_subset::value(config, "agentsync_version").replace('"', "");
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return Ok(());
    }
    let hint = [
        format!("  • Match the pin:  agentsync update {pinned}"),
        format!(
            "  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)"
        ),
    ];
    if outputs_mode(config) == "committed" {
        s.log.error(&format!(
            "This project pins agentsync {pinned} but you are running {engine} — committed outputs must come from one version everywhere."
        ));
        for line in hint {
            s.log.err(line);
        }
        return Err(Stop(1));
    }
    s.log.warning(&format!(
        "This project pins agentsync {pinned} but you are running {engine}."
    ));
    for line in hint {
        s.log.out(line);
    }
    Ok(())
}

/// `list_all_tools`, plus the enabled and profile-tool sets `warm_*_cache` build.
fn load_tools(s: &mut Session, run: &mut Run) {
    let tools_dir = format!("{}/.ai/src/tools", s.paths.root);
    let mut all: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    if let Some(text) = &run.config {
        run.enabled.extend(yaml_subset::list(text, "tools.enabled"));
        run.profile_tools.extend(profiles::all_tools(text));
    }
    for name in s.ws.glob(&tools_dir) {
        let Some(stem) = name.strip_suffix(".yaml") else {
            continue;
        };
        if stem.starts_with('_') || !s.ws.is_file(&format!("{tools_dir}/{name}")) {
            continue;
        }
        if load_tool(s, stem).user_value("enabled") == "true" {
            run.enabled.insert(stem.to_string());
        }
        all.insert(stem.to_string());
    }
    run.tools = all.into_iter().collect();
}

/// The layered tool with its `.ai/src/tools/<slug>.yaml` read from the workspace.
fn load_tool(s: &Session, slug: &str) -> Tool {
    let path = format!("{}/.ai/src/tools/{slug}.yaml", s.paths.root);
    let user_yaml =
        s.ws.read(&path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    Tool::new(slug, user_yaml)
}

/// `_collect_protected_dests`: cleanup never removes what an enabled tool or
/// any profile tool claims. Disabled tools' dests are resolved too, for the
/// log lines `_collect_tool_backup_dests` prints.
fn collect_protected_dests(s: &mut Session, run: &mut Run) {
    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        let tool = load_tool(s, slug);
        if run.enabled.contains(slug) {
            collect_tool_dests(s, run, &tool);
        } else if run.cleanup == "true" {
            for key in TARGET_KEYS {
                let raw = tool.value(&format!("targets.{key}.dest"));
                if !raw.is_empty() {
                    let label = format!("targets.{key}.dest for {slug}");
                    s.paths.clone().resolve_dest(&raw, &label, &mut s.log);
                }
            }
        }
    }
    for slug in run.profile_tools.clone() {
        let tool = load_tool(s, &slug);
        collect_tool_dests(s, run, &tool);
    }
}

fn collect_tool_dests(s: &mut Session, run: &mut Run, tool: &Tool) {
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {}", tool.slug);
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        if s.paths.to_repo_relative(&abs).is_none() {
            s.log
                .error(&format!("Path is outside repository root: {abs}"));
        }
        run.protected.push(abs);
    }
}

/// `_resolve_one_dest`.
fn resolve_one_dest(s: &mut Session, tool: &Tool, key: &str, display: &str) -> String {
    if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
        return String::new();
    }
    let raw = tool.value(&format!("targets.{key}.dest"));
    if raw.is_empty() {
        return String::new();
    }
    let label = format!("targets.{key}.dest for {display}");
    s.paths
        .clone()
        .resolve_dest(&raw, &label, &mut s.log)
        .unwrap_or_default()
}

fn resolve_dests(s: &mut Session, tool: &Tool, display: &str) -> Dests {
    Dests {
        agents: resolve_one_dest(s, tool, "agents", display),
        rules: resolve_one_dest(s, tool, "rules", display),
        skills: resolve_one_dest(s, tool, "skills", display),
        commands: resolve_one_dest(s, tool, "commands", display),
        subagents: resolve_one_dest(s, tool, "subagents", display),
        settings: resolve_one_dest(s, tool, "settings", display),
        mcp: resolve_one_dest(s, tool, "mcp", display),
        hooks: resolve_one_dest(s, tool, "hooks", display),
        guard: resolve_one_dest(s, tool, "guard", display),
    }
}

/// `resolve_source_path` as the steps call it: an unsafe root ends the run.
fn source_path(s: &mut Session, raw: &str, label: &str) -> Result<String, Stop> {
    s.paths
        .clone()
        .resolve_source(raw, label, &mut s.log)
        .ok_or(Stop(1))
}

/// `_resolve_tool_src`.
fn tool_source(
    s: &mut Session,
    tool: &Tool,
    key: &str,
    default: &str,
    display: &str,
) -> Result<String, Stop> {
    let configured = tool.value(&format!("targets.{key}.source"));
    let raw = if configured.is_empty() {
        default
    } else {
        &configured
    };
    source_path(s, raw, &format!("targets.{key}.source for {display}"))
}

/// `sync_tool`.
fn sync_tool(s: &mut Session, run: &mut Run, slug: &str) -> Step {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    run.printed = true;
    let dests = resolve_dests(s, &tool, &display);
    s.log.info(&format!("Syncing {display}..."));

    if !dests.agents.is_empty() {
        let src = tool_source(s, &tool, "agents", &run.sources.agents, &display)?;
        file_ops::copy_file(s, &src, &dests.agents).map_err(|e| io(s, e))?;
    }
    sync_rules_step(s, run, &tool, &dests, &display)?;
    sync_skills_step(s, run, &tool, &dests, &display)?;
    sync_commands_step(s, run, &tool, &dests, &display)?;
    sync_subagents_step(s, run, &tool, &dests, &display)?;
    sync_payloads_step(s, &tool, &dests)?;

    if !tool.value("post_sync").is_empty() {
        s.log.info(&format!(
            "Skipping post-sync hook for {display} (AGENTSYNC_SKIP_POST_SYNC=true)"
        ));
    }
    s.log.success(&format!("{display} complete"));
    Ok(())
}

fn sync_rules_step(s: &mut Session, run: &Run, tool: &Tool, dests: &Dests, display: &str) -> Step {
    let src_agents = tool_source(s, tool, "agents", &run.sources.agents, display)?;
    let src_rules = tool_source(s, tool, "rules", &run.sources.rules, display)?;
    let include = tool.filter("targets.rules.include");
    let exclude = tool.filter("targets.rules.exclude");

    if tool.value("targets.rules.inline_into_agents") == "true" && !dests.agents.is_empty() {
        inline_rules_into_agents(s, &src_rules, &dests.agents, &include, &exclude)?;
    } else if !dests.rules.is_empty() {
        if tool.value("targets.rules.merge_to_file") == "true" {
            let prepend = (tool.value("targets.rules.prepend_agents") == "true"
                && s.ws.is_file(&src_agents))
            .then_some(src_agents.as_str());
            rules::merge_rules_to_file(s, &src_rules, &dests.rules, &include, &exclude, prepend)
                .map_err(|e| io(s, e))?;
        } else {
            let extension = tool.value("targets.rules.extension");
            let header = tool.value("targets.rules.header");
            let scoped_header = tool.value("targets.rules.scoped_header");
            let opts = RuleOptions {
                extension: &extension,
                header: &header,
                scoped_header: &scoped_header,
                include: &include,
                exclude: &exclude,
            };
            rules::sync_rules(s, &src_rules, &dests.rules, &opts).map_err(|e| io(s, e))?;
            if tool.value("targets.rules.append_imports") == "true" {
                if dests.agents.is_empty() {
                    s.log.warning(&format!(
                        "Skipping append_imports for {display} because targets.agents.dest is missing"
                    ));
                } else {
                    rules::append_imports(s, &dests.agents, &dests.rules).map_err(|e| io(s, e))?;
                    s.log.step(&format!(
                        "Appended @rules imports to {}",
                        paths::leaf(&dests.agents)
                    ));
                }
            }
        }
    }

    if !dests.agents.is_empty()
        && !dests.rules.is_empty()
        && dests.agents.starts_with(&format!("{}/", dests.rules))
    {
        let _ = file_ops::copy_file(s, &src_agents, &dests.agents);
    }
    Ok(())
}

/// `_inline_rules_into_agents`.
fn inline_rules_into_agents(
    s: &mut Session,
    src_rules: &str,
    dest_agents: &str,
    include: &str,
    exclude: &str,
) -> Step {
    if !s.ws.is_dir(src_rules) {
        return Ok(());
    }
    let mut block = "\n\n## Rules\n\nThe following rule files define project constraints. Read them before making changes:\n\n"
        .as_bytes()
        .to_vec();
    for name in s.ws.glob(src_rules) {
        let path = format!("{src_rules}/{name}");
        if !name.ends_with(".md")
            || !s.ws.is_file(&path)
            || !crate::filters::matches(&name, include, exclude)
        {
            continue;
        }
        let bytes = s.ws.read(&path).map_err(|e| io(s, e))?;
        let title = crate::text::lines(&bytes)
            .into_iter()
            .find(|line| line.starts_with(b"#"))
            .map(|line| {
                let without_hashes = &line[line.iter().take_while(|b| **b == b'#').count()..];
                let spaces = without_hashes.iter().take_while(|b| **b == b' ').count();
                without_hashes[spaces..].to_vec()
            })
            .unwrap_or_default();
        block.extend_from_slice(format!("- `{name}` — ").as_bytes());
        block.extend(title);
        block.push(b'\n');
    }
    block.extend_from_slice("\nFind all rules in `.ai/src/rules/`.\n".as_bytes());
    s.ws.append(dest_agents, &block).map_err(|e| io(s, e))?;
    s.record_write(dest_agents);
    s.log.step(&format!(
        "Appended rule references to {}",
        paths::leaf(dest_agents)
    ));
    Ok(())
}

fn skill_description(skill: &[u8]) -> Vec<u8> {
    let lines = crate::text::lines(skill);
    let mut in_range = false;
    let mut first = Vec::new();
    for line in &lines {
        if !in_range {
            in_range = *line == b"---";
            continue;
        }
        if *line == b"---" {
            in_range = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"description:") {
            let rest = crate::text::trim_start_space(rest);
            let rest = match rest.strip_prefix(b">") {
                Some(after) => crate::text::trim_start_space(after),
                None => rest,
            };
            first = rest.to_vec();
            break;
        }
    }
    if first == b">" {
        first.clear();
    }
    if !first.is_empty() {
        return first;
    }
    let mut outer = false;
    let mut inner = false;
    for line in &lines {
        let mut closes_outer = false;
        if !outer {
            if *line != b"---" {
                continue;
            }
            outer = true;
        } else if *line == b"---" {
            closes_outer = true;
        }
        if !inner {
            inner = line.starts_with(b"description:");
        } else if line.first().is_some_and(u8::is_ascii_lowercase) {
            inner = false;
        } else if line.starts_with(b"  ") {
            return crate::text::trim_start_space(line).to_vec();
        }
        if closes_outer {
            outer = false;
        }
    }
    Vec::new()
}

/// `_inline_skills_into_file`.
fn inline_skills_into_file(
    s: &mut Session,
    src_skills: &str,
    target: &str,
    include: &str,
    exclude: &str,
) -> Step {
    let mut entries = Vec::new();
    for name in s.ws.glob(src_skills) {
        let dir = format!("{src_skills}/{name}");
        if !s.ws.is_dir(&dir) || !crate::filters::matches(&name, include, exclude) {
            continue;
        }
        let skill_file = format!("{dir}/SKILL.md");
        let desc = if s.ws.is_file(&skill_file) {
            skill_description(&s.ws.read(&skill_file).map_err(|e| io(s, e))?)
        } else {
            Vec::new()
        };
        entries.extend_from_slice(format!("- `{name}`").as_bytes());
        if !desc.is_empty() {
            entries.extend_from_slice(" — ".as_bytes());
            entries.extend(desc);
        }
        entries.push(b'\n');
    }
    if entries.is_empty() {
        return Ok(());
    }
    let mut block = "\n## Skills\n\nThe following skills provide step-by-step workflows. Find them in `.ai/src/skills/`:\n\n"
        .as_bytes()
        .to_vec();
    block.extend(entries);
    s.ws.append(target, &block).map_err(|e| io(s, e))?;
    s.record_write(target);
    s.log
        .step(&format!("Appended skill index to {}", paths::leaf(target)));
    Ok(())
}

fn sync_skills_step(s: &mut Session, run: &Run, tool: &Tool, dests: &Dests, display: &str) -> Step {
    let src_skills = tool_source(s, tool, "skills", &run.sources.skills, display)?;
    let include = tool.filter("targets.skills.include");
    let exclude = tool.filter("targets.skills.exclude");

    if !dests.skills.is_empty() {
        let effective = if exclude.is_empty() {
            "command-*".to_string()
        } else {
            format!("{exclude} command-*")
        };
        return file_ops::sync_dir(s, &src_skills, &dests.skills, &include, &effective)
            .map_err(|e| io(s, e));
    }
    if tool.value("targets.skills.inline_into_agents") == "true" && s.ws.is_dir(&src_skills) {
        let target = if !dests.agents.is_empty() {
            dests.agents.clone()
        } else if tool.value("targets.rules.merge_to_file") == "true" && s.ws.is_file(&dests.rules)
        {
            dests.rules.clone()
        } else {
            String::new()
        };
        if !target.is_empty() {
            inline_skills_into_file(s, &src_skills, &target, &include, &exclude)?;
        }
    }
    Ok(())
}

fn sync_commands_step(
    s: &mut Session,
    run: &Run,
    tool: &Tool,
    dests: &Dests,
    display: &str,
) -> Step {
    if run.sources.commands.is_empty() {
        return Ok(());
    }
    let include = tool.filter("targets.commands.include");
    let exclude = tool.filter("targets.commands.exclude");
    let label = format!("source.commands for {display}");
    let src = source_path(s, &run.sources.commands, &label)?;
    if !s.ws.is_dir(&src) {
        return Ok(());
    }

    if !dests.commands.is_empty() {
        let result = if tool.value("targets.commands.format") == "toml" {
            rules::sync_converted(s, &src, &dests.commands, Conversion::CommandToml)
        } else {
            let extension = tool.value("targets.commands.extension");
            let opts = RuleOptions {
                extension: &extension,
                header: "",
                scoped_header: "",
                include: "",
                exclude: "",
            };
            rules::sync_rules(s, &src, &dests.commands, &opts)
        };
        return result.map_err(|e| io(s, e));
    }
    if tool.value("targets.commands.as_skills") == "true" && !dests.skills.is_empty() {
        s.log.info(&format!(
            "{display} has no native commands surface — generating skills (command-*) instead"
        ));
        return rules::sync_commands_as_skills(s, &src, &dests.skills, &include, &exclude)
            .map_err(|e| io(s, e));
    }
    if tool.value("targets.commands.inline_into_agents") == "true" {
        let target = if !dests.agents.is_empty() {
            dests.agents.clone()
        } else if tool.value("targets.rules.merge_to_file") == "true" && s.ws.is_file(&dests.rules)
        {
            dests.rules.clone()
        } else {
            String::new()
        };
        if !target.is_empty() {
            s.log.info(&format!(
                "{display} has no native commands surface — appending command index to {}",
                paths::leaf(&target)
            ));
            rules::inline_commands_to_file(s, &src, &target, &include, &exclude)
                .map_err(|e| io(s, e))?;
        }
    }
    Ok(())
}

fn sync_subagents_step(
    s: &mut Session,
    run: &Run,
    tool: &Tool,
    dests: &Dests,
    display: &str,
) -> Step {
    if dests.subagents.is_empty() || run.sources.subagents.is_empty() {
        return Ok(());
    }
    let label = format!("source.subagents for {display}");
    let src = source_path(s, &run.sources.subagents, &label)?;
    if !s.ws.is_dir(&src) {
        return Ok(());
    }
    let result = match tool.value("targets.subagents.format").as_str() {
        "toml" => rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentToml),
        "amazonq_json" => {
            rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentAmazonqJson)
        }
        "opencode_md" => {
            rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentOpencodeMd)
        }
        _ => {
            let extension = tool.value("targets.subagents.extension");
            let opts = RuleOptions {
                extension: &extension,
                header: "",
                scoped_header: "",
                include: "",
                exclude: "",
            };
            rules::sync_rules(s, &src, &dests.subagents, &opts)
        }
    };
    result.map_err(|e| io(s, e))
}

fn sync_payloads_step(s: &mut Session, tool: &Tool, dests: &Dests) -> Step {
    let root = s.paths.root.clone();
    let src_settings = if dests.settings.is_empty() {
        None
    } else {
        payload::resolve_source(s, tool, "settings")
    }
    .filter(|path| s.ws.is_file(path));
    let src_mcp = if dests.mcp.is_empty() {
        None
    } else {
        payload::resolve_source(s, tool, "mcp")
    }
    .filter(|path| s.ws.is_file(path));

    if tool.value("targets.mcp.format") == "opencode_json" {
        if let Some(settings) = &src_settings {
            if let Some(mcp) = &src_mcp {
                compose_opencode(s, settings, mcp, &dests.settings)?;
                let label = payload::describe_source(&root, mcp, &tool.slug, "mcp");
                if !label.is_empty() {
                    s.log.step(&format!("mcp source: {label}"));
                }
            } else {
                file_ops::copy_file(s, settings, &dests.settings).map_err(|e| io(s, e))?;
            }
        }
    } else {
        if let Some(settings) = &src_settings {
            file_ops::copy_file(s, settings, &dests.settings).map_err(|e| io(s, e))?;
        }
        if let Some(mcp) = &src_mcp {
            let label = payload::describe_source(&root, mcp, &tool.slug, "mcp");
            file_ops::copy_file(s, mcp, &dests.mcp).map_err(|e| io(s, e))?;
            if !label.is_empty() {
                s.log.step(&format!("mcp source: {label}"));
            }
        }
    }

    for (resource, dest) in [("hooks", &dests.hooks), ("guard", &dests.guard)] {
        if dest.is_empty() {
            continue;
        }
        if let Some(src) = payload::resolve_source(s, tool, resource).filter(|p| s.ws.is_file(p)) {
            file_ops::copy_file(s, &src, dest).map_err(|e| io(s, e))?;
        }
    }
    Ok(())
}

/// `sync_opencode_config`.
fn compose_opencode(s: &mut Session, settings: &str, mcp: &str, dest: &str) -> Step {
    let settings_text =
        String::from_utf8_lossy(&s.ws.read(settings).map_err(|e| io(s, e))?).into_owned();
    let mcp_text = String::from_utf8_lossy(&s.ws.read(mcp).map_err(|e| io(s, e))?).into_owned();
    match opencode_json::compose(&settings_text, &mcp_text) {
        Err(failure) => {
            let (settings_disp, mcp_disp) = (s.display(settings), s.display(mcp));
            s.log.error(&format!(
                "Cannot compose OpenCode config from {settings_disp} and {mcp_disp}: {}",
                failure.message
            ));
            Err(Stop(failure.code))
        }
        Ok(composed) => {
            s.ws.create_dir_all(&paths::parent(dest));
            s.ws.remove(dest);
            s.ws.write(dest, composed.into_bytes())
                .map_err(|e| io(s, e))?;
            s.record_write(dest);
            let line = format!(
                "{} + {} → {}",
                s.display(settings),
                s.display(mcp),
                s.display(dest)
            );
            s.log.step(&line);
            Ok(())
        }
    }
}

/// `cleanup_tool`: remove a disabled tool's unprotected outputs.
fn cleanup_tool(s: &mut Session, run: &mut Run, slug: &str) {
    run.printed = false;
    if run.cleanup != "true" {
        return;
    }
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    let mut cleaned = false;
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {display}");
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        if !run.protected.contains(&abs) && file_ops::cleanup_path(s, &abs) {
            cleaned = true;
        }
    }
    if cleaned {
        s.log.info(&format!("Cleaned up {display} (disabled)"));
        run.printed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    fn project() -> Session {
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

    #[test]
    fn a_project_without_agents_md_stops_with_status_one() {
        let mut s = test_session();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(2),
            [
                "[ERROR] Source agents file not found: /proj/.ai/src/AGENTS.md",
                "[ERROR] Run 'agentsync init' or set source.agents in agent_sync.yaml"
            ]
        );
    }

    #[test]
    fn an_unknown_outputs_mode_stops_before_the_banner() {
        let mut s = project();
        file(&mut s, "/proj/.ai/agent_sync.yaml", "outputs: shared\n");
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(1),
            [
                "[ERROR] Unknown outputs mode 'shared' in .ai/agent_sync.yaml — expected 'committed' or 'local'"
            ]
        );
    }

    #[test]
    fn claude_renders_agents_rules_commands_payloads_and_the_engine_skill() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        render(&mut s, &Env::default()).unwrap();
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
        assert_eq!(text_of(&s, "/proj/.claude/rules/core.md"), "# Core\n");
        assert!(s.ws.is_file("/proj/.claude/commands/review.md"));
        assert!(s.ws.is_file("/proj/.claude/skills/agentsync/SKILL.md"));
        assert!(s.ws.is_file("/proj/.claude/settings.json"));
        assert!(s.ws.is_file("/proj/.mcp.json"));
        assert!(s.ws.is_file("/proj/.claude/hooks/agentsync-guard.sh"));
        let touched: Vec<&str> = s.touched().iter().map(String::as_str).collect();
        assert!(touched.contains(&".claude/skills/agentsync/references/maintenance.md"));
        assert!(!touched.contains(&"AGENTS.md"));
    }

    #[test]
    fn a_disabled_tool_is_cleaned_unless_an_enabled_tool_claims_the_dest() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [cursor]\n",
        );
        file(&mut s, "/proj/.codex/agents/x.toml", "x");
        render(&mut s, &Env::default()).unwrap();
        assert!(!s.ws.exists("/proj/.codex/agents"));
        assert!(s.ws.is_file("/proj/AGENTS.md"));
        assert!(
            s.log
                .lines()
                .iter()
                .any(|(_, l)| l == "[INFO] Cleaned up OpenAI Codex (disabled)")
        );
    }

    #[test]
    fn inline_indexes_follow_the_agents_copy_for_codex() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [codex]\n",
        );
        render(&mut s, &Env::default()).unwrap();
        let agents = text_of(&s, "/proj/AGENTS.md");
        assert!(agents.starts_with("# Agents\n\n\n## Rules\n"));
        assert!(agents.contains("- `core.md` — Core\n"));
        assert!(s.ws.is_file("/proj/.agents/skills/command-review/SKILL.md"));
    }

    // Design spec, "Known quirks", item 11: `description: >-` indexes as `-`.
    #[test]
    fn skill_descriptions_come_from_the_frontmatter_scalar_or_its_first_folded_line() {
        assert_eq!(
            skill_description(b"---\nname: a\ndescription: Does A\n---\n"),
            b"Does A"
        );
        assert_eq!(
            skill_description(b"---\ndescription: >\n  Folded first\n  second\nname: x\n---\n"),
            b"Folded first"
        );
        assert_eq!(skill_description(b"---\ndescription: >-\n  x\n---\n"), b"-");
        assert_eq!(skill_description(b"no frontmatter\n"), b"");
    }

    #[test]
    fn an_active_profile_renders_its_variant_under_its_overlay() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nprofiles:\n  hub:\n    active: true\n    tools: [claude-hub]\n",
        );
        file(
            &mut s,
            "/proj/.ai/src/tools/claude-hub.yaml",
            "base: claude\ntargets:\n  agents:\n    dest: \".claude-hub/CLAUDE.md\"\n  rules:\n    dest: \".claude-hub/rules\"\n",
        );
        file(&mut s, "/proj/.ai/profiles/hub/src/rules/hub.md", "# Hub\n");
        render(&mut s, &Env::default()).unwrap();
        assert!(s.ws.is_file("/proj/.claude-hub/rules/hub.md"));
        assert!(s.ws.is_file("/proj/.claude-hub/rules/core.md"));
        assert!(!s.ws.exists("/proj/.claude/rules/hub.md"));
        assert!(!s.ws.exists("/<agentsync-overlay>/profile"));
    }
}
```

- [x] **Step 2: Register the module in `src/lib.rs`**

```rust
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
```

- [x] **Step 3: Record the quirk in the design spec**

After item 10 under "Known quirks", add:

```markdown
11. The inline skill index strips `>` from `description: >-` and indexes the
    skill with the description `-`.
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test --lib`
Expected: `106 passed` (99 + 7 `render`).

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/render.rs src/lib.rs docs/specs/2026-09-12-rust-migration-design.md
git commit -m "feat(native): render sync outputs into the workspace"
```

---

### Task 9: `check` Served Natively

**Files:**
- Create: `src/cli/check.rs`
- Modify: `src/cli/mod.rs` (the `Check` variant)
- Modify: `src/main.rs` (exit status from the command, `project_root`)
- Modify: `bin/agentsync.sh:280` (`_NATIVE_COMMANDS`)
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` ("Accepted deviations")

**Interfaces:**
- Consumes: `render` (Task 8), `overlay::{shared_parent_src, inherit_categories, merge_shared_parent}` (Task 7).
- Produces: `cli::check::check(root: &str, &Env) -> Result<Report, Error>`; `cli::check::Report { pub stdout: String, pub stderr: String, pub status: u8 }`; `cli::check::run(root, &Env, out, err) -> Result<u8, Error>`; `Command::Check`; `main::run` returning the process status. From this task, `agentsync check` with a built binary is answered natively.

- [x] **Step 1: Write `src/cli/check.rs`**

```rust
//! `agentsync check`: render what `sync --force` would write and compare every
//! managed output with the project, with the messages and exit codes of
//! `lib/check.sh`. Nothing is copied and nothing is written.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::paths::Paths;
use crate::render::{self, Env};
use crate::session::Session;
use crate::workspace::Workspace;
use crate::{Error, engine_version, overlay, yaml_subset};

const MANIFEST_REL: &str = ".ai/.sync-manifest";

pub fn run(root: &str, env: &Env, out: &mut impl Write, err: &mut impl Write) -> Result<u8, Error> {
    let report = check(root, env)?;
    out.write_all(report.stdout.as_bytes())
        .map_err(|e| Error::io("<stdout>", e))?;
    err.write_all(report.stderr.as_bytes())
        .map_err(|e| Error::io("<stderr>", e))?;
    Ok(report.status)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub stdout: String,
    pub stderr: String,
    pub status: u8,
}

impl Report {
    fn out(&mut self, line: &str) {
        self.stdout.push_str(line);
        self.stdout.push('\n');
    }

    fn err(&mut self, line: &str) {
        self.stderr.push_str(line);
        self.stderr.push('\n');
    }
}

pub fn check(root: &str, env: &Env) -> Result<Report, Error> {
    let mut report = Report::default();
    if let Some(message) = version_pin_mismatch(root)? {
        for line in message {
            report.err(&line);
        }
        report.status = 1;
        return Ok(report);
    }
    report.out("Checking AgentSync configuration synchronization...");

    let manifest = manifest_paths(root)?;
    let ws = match seed_workspace(root, &manifest) {
        Ok(ws) => ws,
        Err(detail) => {
            report.out("❌ Failed to prepare temporary workspace for check");
            report.err(&detail);
            report.status = 1;
            return Ok(report);
        }
    };

    let mut session = Session::new(ws, Paths::for_disk_root(root));
    merge_shared_parent(&mut session.ws, root)?;
    if render::render(&mut session, env).is_err() {
        report.out("❌ Sync script failed during check");
        report.out("Sync output (last 40 lines):");
        for line in session.log.tail(40) {
            report.out(line);
        }
        report.status = 1;
        return Ok(report);
    }

    let mut compare: BTreeSet<String> = manifest.into_iter().collect();
    for rel in session.touched() {
        if session.ws.is_file(&format!("{root}/{rel}")) {
            compare.insert(rel.clone());
        }
    }

    let mut differences = Vec::new();
    for rel in compare {
        let expected = format!("{root}/{rel}");
        let actual = Path::new(root).join(&rel);
        match (session.ws.is_file(&expected), actual.is_file()) {
            (true, true) => {
                let rendered = session.ws.read(&expected)?;
                let on_disk = std::fs::read(&actual).map_err(|e| Error::io(&actual, e))?;
                if rendered != on_disk {
                    differences.push(format!("Files {rel} differ"));
                }
            }
            (true, false) => differences.push(format!("Missing: {rel}")),
            (false, true) => differences.push(format!("No longer generated: {rel}")),
            (false, false) => {}
        }
    }

    if differences.is_empty() {
        report.out("✅ AgentSync configurations are safe and synced.");
        return Ok(report);
    }
    report.out("");
    report.out("⚠️  AgentSync configurations are out of sync with source.");
    report.out("Differences detected (showing up to 20):");
    for line in differences.iter().take(20) {
        report.out(line);
    }
    report.out("");
    report.out("Please run: lib/sync.sh");
    report.status = 1;
    Ok(report)
}

/// The config `lib/check.sh` reads: `.ai/agent_sync.yaml`, else a root-level one.
fn project_config(root: &str) -> Result<Option<String>, Error> {
    for rel in [".ai/agent_sync.yaml", "agent_sync.yaml"] {
        let path = Path::new(root).join(rel);
        if path.is_file() {
            let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
            return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(None)
}

/// `_check_version_pin`: fatal only for committed outputs.
fn version_pin_mismatch(root: &str) -> Result<Option<Vec<String>>, Error> {
    let Some(config) = project_config(root)? else {
        return Ok(None);
    };
    if yaml_subset::value(&config, "outputs").replace('"', "") != "committed" {
        return Ok(None);
    }
    let pinned = yaml_subset::value(&config, "agentsync_version").replace('"', "");
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return Ok(None);
    }
    Ok(Some(vec![
        format!(
            "❌ This project pins agentsync {pinned} but you are running {engine} — committed outputs must come from one version everywhere."
        ),
        format!("  • Match the pin:  agentsync update {pinned}"),
        format!(
            "  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)"
        ),
    ]))
}

/// `manifest_paths`: the text before the first tab of every complete line.
fn manifest_paths(root: &str) -> Result<Vec<String>, Error> {
    let path = Path::new(root).join(MANIFEST_REL);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    Ok(lines
        .into_iter()
        .map(|line| line.trim_start_matches('\t'))
        .map(|line| line.split('\t').next().unwrap_or(""))
        .filter(|rel| !rel.is_empty())
        .map(str::to_string)
        .collect())
}

/// What `lib/check.sh` copied with `tar`: `.ai/` without backups, a root
/// `agent_sync.yaml`, and every manifest output that exists.
fn seed_workspace(root: &str, manifest: &[String]) -> Result<Workspace, String> {
    let mut ws = Workspace::new(root);
    let ai = Path::new(root).join(".ai");
    if !ai.exists() {
        return Err("Incomplete copy — missing: .ai".to_string());
    }
    let skip_in_ai = |rel: &str| rel == "backups" || rel.starts_with("backups/") || is_git(rel);
    ws.seed_from_disk(&format!("{root}/.ai"), &ai, &skip_in_ai)
        .map_err(|e| e.to_string())?;
    let config = Path::new(root).join("agent_sync.yaml");
    if config.is_file() {
        ws.seed_from_disk(&format!("{root}/agent_sync.yaml"), &config, &is_git)
            .map_err(|e| e.to_string())?;
    }
    for rel in manifest {
        if rel.starts_with(".ai/") || is_git(rel) {
            continue;
        }
        let disk = Path::new(root).join(rel);
        if disk.exists() {
            ws.seed_from_disk(&format!("{root}/{rel}"), &disk, &is_git)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(ws)
}

fn is_git(rel: &str) -> bool {
    rel.split('/').any(|segment| segment == ".git")
}

/// The `shared:` block of `lib/check.sh`, before the render.
fn merge_shared_parent(ws: &mut Workspace, root: &str) -> Result<(), Error> {
    let Some(config) = project_config(root)? else {
        return Ok(());
    };
    if let Some(parent) = overlay::shared_parent_src(&config, root) {
        let inherit = yaml_subset::value(&config, "shared.inherit");
        overlay::merge_shared_parent(ws, &parent, &overlay::inherit_categories(&inherit));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        write(&root, ".ai/src/AGENTS.md", "# Agents\n");
        write(
            &root,
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let root = root.to_string_lossy().into_owned();
        (dir, root)
    }

    #[test]
    fn a_project_never_synced_reports_every_output_missing() {
        let (_dir, root) = project();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking AgentSync configuration synchronization...\n\n⚠️  AgentSync configurations are out of sync with source.\nDifferences detected (showing up to 20):\n"));
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
        assert!(report.stdout.ends_with("\nPlease run: lib/sync.sh\n"));
    }

    #[test]
    fn manifest_outputs_that_match_the_render_are_in_sync() {
        let (_dir, root) = project();
        let mut session = Session::new(
            seed_workspace(&root, &[]).unwrap(),
            Paths::for_disk_root(&root),
        );
        render::render(&mut session, &Env::default()).unwrap();
        let mut manifest = String::new();
        for rel in session.touched() {
            let bytes = session.ws.read(&format!("{root}/{rel}")).unwrap();
            write(Path::new(&root), rel, &String::from_utf8(bytes).unwrap());
            manifest.push_str(&format!("{rel}\thash\n"));
        }
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let clean = check(&root, &Env::default()).unwrap();
        assert_eq!(
            clean.stdout,
            "Checking AgentSync configuration synchronization...\n✅ AgentSync configurations are safe and synced.\n"
        );
        assert_eq!(clean.status, 0);

        write(Path::new(&root), "CLAUDE.md", "edited\n");
        write(Path::new(&root), ".cursor/rules/core.mdc", "stale\n");
        manifest.push_str(".cursor/rules/core.mdc\thash\n");
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let dirty = check(&root, &Env::default()).unwrap();
        assert!(dirty.stdout.contains("Files CLAUDE.md differ\n"));
        assert!(
            dirty
                .stdout
                .contains("No longer generated: .cursor/rules/core.mdc\n")
        );
    }

    #[test]
    fn a_committed_pin_mismatch_fails_before_the_banner() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "outputs: committed\nagentsync_version: \"0.0.1\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert_eq!(report.stdout, "");
        assert!(
            report
                .stderr
                .starts_with("❌ This project pins agentsync 0.0.1 but you are running ")
        );
    }

    #[test]
    fn a_failed_render_prints_the_log_tail() {
        let (_dir, root) = project();
        std::fs::remove_file(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking AgentSync configuration synchronization...\n❌ Sync script failed during check\nSync output (last 40 lines):\n[ERROR] Source agents file not found: "));
    }

    #[test]
    fn a_missing_ai_directory_fails_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(
            report,
            Report {
                stdout: "Checking AgentSync configuration synchronization...\n❌ Failed to prepare temporary workspace for check\n".into(),
                stderr: "Incomplete copy — missing: .ai\n".into(),
                status: 1,
            }
        );
    }

    #[test]
    fn manifest_lines_keep_the_text_before_the_first_tab_of_complete_lines() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            MANIFEST_REL,
            "a.md\th\n\tb.md\th\nc.md\nlast\th",
        );
        assert_eq!(manifest_paths(&root).unwrap(), ["a.md", "b.md", "c.md"]);
    }
}
```

- [x] **Step 2: Replace `src/cli/mod.rs`**

```rust
pub mod check;
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
    /// Verify generated outputs match what sync would write.
    #[command(disable_help_flag = true)]
    Check,
}
```

- [x] **Step 3: Replace `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::project::Project;
use agentsync::render::Env;
use agentsync::style::Style;
use agentsync::{Error, engine_version, paths};
use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(status) => ExitCode::from(status),
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            // Bash decides colour from stdout even for stderr lines; same here.
            eprintln!("{}: {e}", Style::for_stdout().red("Error"));
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<u8, Error> {
    guard_engine_version()?;
    // The Bash CLI accepts these flags as commands; clap's own version flag is off.
    if matches!(
        args.first().and_then(|a| a.to_str()),
        Some("--version" | "-v")
    ) {
        return print_version();
    }
    let cli = Cli::parse_from(std::iter::once(OsString::from("agentsync")).chain(args));
    match cli.command {
        Command::Version => print_version(),
        Command::List => {
            let project = Project::discover()?;
            let mut out = std::io::stdout().lock();
            cli::list::run(&project, &Style::for_stdout(), &mut out).map(|()| 0)
        }
        Command::Check => {
            let root = project_root()?;
            let env = Env {
                config_path: std::env::var("AGENTSYNC_CONFIG_PATH").ok(),
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
    }
}

/// `REPO_ROOT` as `lib/check.sh` derives it: `AGENTSYNC_REPO_ROOT`, else the
/// working directory, spelled logically.
fn project_root() -> Result<String, Error> {
    let env_root = std::env::var("AGENTSYNC_REPO_ROOT")
        .ok()
        .filter(|root| !root.is_empty());
    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
    let root = paths::logical_root(
        env_root.as_deref(),
        &cwd,
        std::env::var("PWD").ok().as_deref(),
    );
    if !std::path::Path::new(&root).is_dir() {
        return Err(Error::ProjectRootNotFound(PathBuf::from(
            env_root.unwrap_or(root),
        )));
    }
    Ok(root)
}

fn print_version() -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
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

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test`
Expected: `112 passed` (106 + 6 `cli::check`, all `#[cfg(unix)]`) and the 7 integration tests.

- [x] **Step 5: Declare `check` ported in `bin/agentsync.sh`**

```bash
_NATIVE_COMMANDS=" version --version -v list ls check "
```

`check --help` keeps printing the Bash usage: the dispatcher intercepts `--help` for `check` before `_native_try`.

- [x] **Step 6: Record the accepted deviations in the design spec**

Under "Accepted deviations", after the Phase 1 lines, add:

```markdown
- Phase 2: directory listings and globs read in byte order; Bash globs followed
  the locale's collation, so merged rules, indexes, and imports can order
  differently for names the locale sorts otherwise.
- Phase 2: when `check`'s render fails, the log tail names project paths and
  the virtual `/<agentsync>` and `/<agentsync-overlay>` roots where Bash named
  its random temporary workspace and overlay directories.
- Phase 2: `check` without `.ai/` reports `Incomplete copy — missing: .ai` on
  stderr where Bash printed `tar`'s platform-specific error.
- Phase 2: `check` reads `agent_sync.yaml` with its `shared:` block in place;
  Bash removed the block from a temporary copy. They differ only when another
  key's lookup falls through into that block (quirk 1).
- Phase 2: symlinks under `.ai/` and among the outputs are followed; Bash's
  `tar` copy kept them as links.
- Phase 2: `agent_sync.yaml` and OpenCode JSON that are not valid UTF-8 are read
  with replacement characters; Markdown transforms stay byte-exact.
- Phase 2: `printf '%b'` escapes in rule headers expand as Bash 3.2 does,
  leaving `\u` literal where Bash 5 expanded it.
```

- [x] **Step 7: Prove the bats files that call `check` against the binary**

```bash
cargo build --release
bats tests/check.bats tests/base_skills.bats tests/team_workflow.bats tests/profiles.bats tests/version_pin.bats tests/sync_options.bats
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/check.bats tests/base_skills.bats tests/team_workflow.bats tests/profiles.bats tests/version_pin.bats tests/sync_options.bats --tap | grep -c '^ok'
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: 73 tests pass in Bash (10 + 12 + 8 + 20 + 6 + 17); `73` under `AGENTSYNC_NATIVE=1`; ShellCheck exits 0. `check leaves no temp artifacts behind` passes natively because nothing is written.

- [x] **Step 8: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/check.rs src/cli/mod.rs src/main.rs bin/agentsync.sh docs/specs/2026-09-12-rust-migration-design.md
git commit -m "feat(native): port check"
```

---

### Task 10: Parity Fixtures for `check`, and CI

**Files:**
- Modify: `tests/native_parity.bats` (append the `check` section)
- Modify: `.github/workflows/ci.yaml` (`native` job)

**Interfaces:**
- Consumes: the binary, `assert_parity`, `_run_engine`, `enable_tools`.
- Produces: `assert_parity_head <lines> <args…>` and `_bash_sync` for later phases; 15 fixtures covering the spec's Phase 2 exit list — never-synced and 13-tool golden outputs (`sync.bats` fixtures), edits and deletions, disabled-tool cleanup, `shared`, `base_skills`, `profiles`, `opencode`, `resource_resolver` payload order, filters and every rules option, the version pin, the two failure paths, and this repository's own `.ai/src/` with all 13 tools.

- [x] **Step 1: Append to `tests/native_parity.bats`**

```bash

# ── check ────────────────────────────────────────────────────────────────────
# Outputs always come from the Bash sync, so a native check that agrees proves
# the render reproduces every managed file byte for byte.

ALL_TOOLS=(amazonq antigravity claude cline codex copilot cursor gemini junie kimi opencode windsurf zed)

_bash_sync() { _run_engine 0 sync "$@" >/dev/null 2>&1; }

# Usage: assert_parity_head <lines> <agentsync args...>
# For an accepted deviation past the first <lines> lines: the exit status and
# those lines must match.
assert_parity_head() {
    local count="$1"
    shift
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    bash_out=$(printf '%s\n' "$bash_out" | head -n "$count")
    native_out=$(printf '%s\n' "$native_out" | head -n "$count")
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

_all_tools_fixture() {
    enable_tools "${ALL_TOOLS[@]}"
    printf '%s\n' '---' 'paths:' '  - "**/*.dart"' '---' '' '# Scoped Fixture Rule' '' '- Body.' \
        > .ai/src/rules/scoped-fixture.md
    printf '%s\n' '---' 'description: Explicit-only fixture command' 'disable-model-invocation: true' '---' '' 'Body.' \
        > .ai/src/commands/explicit-only.md
}

@test "parity: check on a project that was never synced" {
    enable_tools claude cursor
    assert_parity check
}

@test "parity: check after a Bash sync of all 13 tools" {
    _all_tools_fixture
    _bash_sync
    assert_parity check
}

@test "parity: check reports edited, deleted, and stale outputs" {
    _all_tools_fixture
    _bash_sync
    echo "edit" >> CLAUDE.md
    rm -f .cursor/rules/core.mdc
    echo "# more" >> .ai/src/rules/core.md
    assert_parity check
}

@test "parity: check after a tool is disabled" {
    enable_tools claude cursor codex
    _bash_sync
    _run_engine 0 disable cursor >/dev/null
    assert_parity check
}

@test "parity: check with shared inheritance and a changed parent" {
    enable_tools claude codex
    mkdir -p 'shared parent/.ai/src/rules' 'shared parent/.ai/src/skills/parent-only' 'shared parent/.ai/src/tools'
    printf 'parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    printf 'parent skill\n' > 'shared parent/.ai/src/skills/parent-only/SKILL.md'
    printf 'targets:\n  agents:\n    dest: "OTHER.md"\n' > 'shared parent/.ai/src/tools/claude.yaml'
    printf '\nshared:\n  path: "shared parent"\n  inherit: rules, skills, tools\n' >> .ai/agent_sync.yaml
    _bash_sync
    assert_parity check
    printf 'changed parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    assert_parity check
}

@test "parity: check with the engine skill layer off and a project copy of it" {
    enable_tools claude kimi
    _bash_sync
    printf '\nbase_skills: false\n' >> .ai/agent_sync.yaml
    assert_parity check
    mkdir -p .ai/src/skills/agentsync
    printf -- '---\nname: agentsync\ndescription: Project copy\n---\n' > .ai/src/skills/agentsync/SKILL.md
    assert_parity check
}

@test "parity: check with an active profile overlay" {
    enable_tools claude
    _run_engine 0 profile add hub --tools claude,codex >/dev/null
    mkdir -p .ai/profiles/hub/src/rules
    printf '# Hub only\n' > .ai/profiles/hub/src/rules/hub.md
    _bash_sync
    assert_parity check
    printf '# Hub changed\n' > .ai/profiles/hub/src/rules/hub.md
    assert_parity check
}

@test "parity: check with composed OpenCode MCP" {
    enable_tools opencode
    mkdir -p .ai/src/tools/opencode
    printf '%s\n' '{"$schema":"https://opencode.ai/config.json","theme":"system"}' > .ai/src/tools/opencode/settings.json
    printf '%s\n' '{"mcpServers":{"github":{"command":"npx","args":["-y","@github/mcp"],"env":{"TOKEN":"${GITHUB_TOKEN}"}},"docs":{"type":"sse","url":"https://example.test/mcp","headers":{"Authorization":"Bearer {env:TOKEN}"},"enabled":false,"timeout":9000,"oauth":false}}}' > .ai/src/mcp.json
    _bash_sync
    assert_parity check
}

@test "parity: check when the OpenCode MCP source is malformed" {
    enable_tools opencode claude
    _bash_sync
    printf '%s\n' '{"mcpServers":[]}' > .ai/src/mcp.json
    assert_parity_head 3 check
}

@test "parity: check with legacy, declared, per-tool, and shared payload sources" {
    enable_tools claude cursor windsurf junie
    mkdir -p .ai/src/mcp .ai/src/tools/cursor config
    echo '{"legacy":true}' > .ai/src/mcp/claude.json
    echo '{"per-tool":true}' > .ai/src/tools/cursor/hooks.json
    echo '{"shared":true}' > .ai/src/mcp.json
    echo '{"declared":true}' > config/windsurf-mcp.json
    printf 'targets:\n  mcp:\n    source: "config/windsurf-mcp.json"\n' > .ai/src/tools/windsurf.yaml
    _bash_sync
    assert_parity check
}

@test "parity: check with skills filters and a custom tool using every rules option" {
    enable_tools claude mytool
    mkdir -p .ai/src/skills/keepme .ai/src/skills/dropme .ai/src/tools
    echo "k" > .ai/src/skills/keepme/SKILL.md
    echo "d" > .ai/src/skills/dropme/SKILL.md
    printf 'targets:\n  skills:\n    exclude:\n      - dropme\n  rules:\n    include: [core.md, git.md]\n' > .ai/src/tools/claude.yaml
    cat > .ai/src/tools/mytool.yaml <<'YAML'
name: "My Tool"
targets:
  agents:
    dest: ".mytool/rules/AGENTS.md"
  rules:
    dest: ".mytool/rules"
    extension: ".txt"
    header: "---\nalways: true\n---"
    append_imports: true
  skills:
    dest: ".mytool/skills"
    include: "keep*"
  commands:
    inline_into_agents: true
  subagents:
    dest: ".mytool/agents"
    format: amazonq_json
YAML
    _bash_sync
    assert_parity check
}

@test "parity: check with a committed version pin mismatch" {
    enable_tools claude
    printf '\noutputs: committed\nagentsync_version: "0.0.1"\n' >> .ai/agent_sync.yaml
    assert_parity check
}

@test "parity: check without AGENTS.md" {
    enable_tools claude
    rm -f .ai/src/AGENTS.md
    assert_parity_head 3 check
}

@test "parity: check without a .ai directory" {
    rm -rf .ai
    assert_parity_head 2 check
}

@test "parity: check with this repository's own .ai/src and all 13 tools" {
    rm -rf .ai/src
    cp -R "$REPO_ROOT/.ai/src" .ai/src
    enable_tools "${ALL_TOOLS[@]}"
    _bash_sync
    assert_parity check
}
```

- [x] **Step 2: Run the parity suite, confirm green**

```bash
cargo build --release
bats --jobs 4 tests/native_parity.bats --tap | grep -c '^ok'
AGENTSYNC_NATIVE_BIN=/nonexistent bats tests/native_parity.bats --tap | grep -c 'skip'
```

Expected: `24` (9 Phase 1 + 15 `check`); `24` skipped without a binary. A failure prints the diff between the engines; fix the native side, never the fixture. The `.ai/src` golden fixture is the slowest — expect minutes serially.

- [x] **Step 3: Extend the `native` CI job**

In `.github/workflows/ci.yaml`, `native` job: raise `timeout-minutes: 20` to `timeout-minutes: 40`, install GNU parallel with bats, and run the files that call `check`:

```yaml
      - name: Install bats-core and GNU parallel
        if: runner.os != 'Windows'
        shell: bash
        run: |
          if [[ "$RUNNER_OS" == "Linux" ]]; then
            sudo apt-get install -y bats parallel
          else
            brew install bats-core parallel
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
        run: >-
          bats --jobs 4 --tap
          tests/cli.bats tests/list.bats tests/native_dispatch.bats tests/native_parity.bats
          tests/check.bats tests/base_skills.bats tests/team_workflow.bats tests/profiles.bats
          tests/version_pin.bats tests/sync_options.bats
```

These replace the job's existing `Install bats-core` and `Ported commands through the Bash suite` steps.

- [x] **Step 4: Run the whole suite in both modes**

```bash
bats --jobs 4 tests/ --tap | head -1
bats --jobs 4 tests/ --tap | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap | grep -c '^not ok'
```

Expected: `1..762` (745 + 1 Task 0b + 1 Task 0c + 15 parity) and `0` failures in both modes.

- [x] **Step 5: Commit**

```bash
git add tests/native_parity.bats .github/workflows/ci.yaml
git commit -m "test(native): diff Bash against native check across the render surface"
```

---

### Task 11: Module Map

**Files:**
- Modify: `.ai/src/skills/native-port/references/module-map.md`
- Modify: `.ai/.sync-manifest` (regenerated)

**Interfaces:**
- Consumes: the modules as they landed.
- Produces: the map the Phase 3 plan is written from.

- [ ] **Step 1: Update the engine rows of `module-map.md`**

Replace these rows in the "Engine modules" block:

```text
lib/helpers/logging.sh           → src/log.rs              plain [INFO]/[SUCCESS]/[WARNING]/[ERROR], step, separator, tail; display_path is in src/paths.rs
(bash builtins)                  → src/text.rs             read loops, [[:space:]], printf '%b', $(...) newlines, JSON/TOML escapes
(check.sh tar copy)              → src/workspace.rs        in-memory tree: disk index, embedded engine at /<agentsync>, overlays at /<agentsync-overlay>
lib/helpers/profiles.sh          → src/profiles.rs         names, overlay dir, tools, active; profile_rewrite_dest waits for `profile`
lib/helpers/manifest.sh          → src/session.rs (record_write, was_touched, record_tree; Phase 2), src/manifest.rs (load, drift, write; Phase 3)
lib/helpers/format_conversion.sh → src/convert.rs          frontmatter and per-file converters; directory loops live in src/rules.rs
lib/helpers/opencode.sh          → src/opencode_json.rs    awk composer, exit codes 20-26
lib/helpers/shared.sh            → src/overlay.rs          base-src and profile overlays, shared parent merge for check
lib/check.sh                     → src/cli/check.rs        render + compare, no tar; Phase 2, ported
lib/sync.sh                      → src/render.rs (forced render, Phase 2) + src/cli/sync.rs (transaction, Phase 3)
```

- [ ] **Step 2: Regenerate this repository's agent files**

Run: `bash bin/agentsync.sh sync`
Expected: `Synced 2/13 tools (11 skipped)`; `git status --short` lists the module map and `.ai/.sync-manifest` (outputs are gitignored here).

- [ ] **Step 3: Commit**

```bash
git add .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest
git commit -m "docs(native): map the phase 2 modules"
```

---

## Completion

Before reporting Phase 2 done, append the completion receipt from `verification.md`:

- Each Global Constraint mapped to the file that satisfies it, including the seven proposed deviations and quirks 9–11 as they read in the spec.
- Fresh output of: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`, ShellCheck over the shell entry points, and `bats --jobs 4 tests/ --tap` under both `AGENTSYNC_NATIVE=0` and `AGENTSYNC_NATIVE=1`.
- Golden outputs on the 13-tool fixture: `bash scripts/perf/make-fixture.sh <dir>`, a Bash `sync` there, then `AGENTSYNC_NATIVE=1 agentsync check` exits 0.
- `check` timed on the same fixture in both engines with `bash scripts/perf/bench.sh --runs 3`, compared with the 63.33 s Bash best in `docs/perf/2026-09-13-bash-baseline.md`.
- The `native` CI job's result on the branch, or the line recorded as open when the branch is still unpushed.
- Anything skipped or deferred.

---

## Run log

### 2026-09-13 — Phase 2 planned
- Commits: `docs(native): plan phase 2`
- Verified: Phase 1's plan has no unchecked box and carries its receipt. Before writing, every Rust snippet in this plan was built and run in a scratch worktree of `48871ba`, and each task was then replayed alone in a second clean worktree: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass after every task, with the unit counts the steps state (46, 54, 60, 69, 73, 89, 99, 106, 112; 7 integration). Converter, rule, and OpenCode expectations were read off the Bash helpers by running them. With `check` in `_NATIVE_COMMANDS` and a release build: `tests/native_parity.bats` → 24 ok; the six bats files that call `check` → 73 ok under `AGENTSYNC_NATIVE=1`; a Bash `sync` of all 13 tools (232 managed outputs) followed by native `check` → exit 0 and byte-identical output, and identical again after an edited output, a deleted output, a changed source, and a disabled tool. The whole suite with all of it in place: `bats --jobs 4 tests/ --tap` → `1..762`, 762 ok, exit 0, and the same under `AGENTSYNC_NATIVE=1`. Both Bash fixes were red before and green after, and the whole suite with them alone ran 746 of 747 green, the one failure being the `shared.bats` fixture Task 0b amends.
- Plan amended: none; this run wrote it.
- Next: the plan's review. Two decisions ride on it: Task 0b's product question (keep `AGENTS.md` required, the recommendation, or let `sync` run without one), and ratifying the seven accepted deviations in the Global Constraints. After approval, Task 0 Step 1.
- Blocker: none. `cargo` is installed at `~/.cargo/bin` but was not on this agent shell's `PATH`; commands ran as `PATH="$HOME/.cargo/bin:$PATH" cargo …`.
