# Rust Migration Phase 3: Native `sync` Transaction and `rollback`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Serve `agentsync sync` — its manifest, drift gate, backup, `.gitignore` block, post-sync hooks, `--dry-run`, `--force`, `--only`, `--skip`, `--profile`, `--if-stale`, `--workspace`, the version pin, the first-sync warning, and the signal-safe restore — and `agentsync rollback` from the Rust binary, writing the same files in the same on-disk formats as `lib/sync.sh` and `lib/helpers/backup.sh`.

**Architecture:** Phase 2's render wrote into an in-memory `Workspace`. For `sync` the same workspace is opened on disk: paths below the project root are read and written in place, the embedded engine and the overlay trees stay virtual, and path containment resolves through the disk so a symlinked directory cannot carry a destination out of the project. The render is split into the stages `lib/sync.sh` runs (`prepare`, pin, banner, overlays, catalog, passes), so `check` keeps composing them in memory while `cli::sync` runs them on disk with the manifest, backup, and `.gitignore` transaction between them, exactly where Bash does; the log streams line by line, coloured on a terminal, so post-sync hooks interleave as they do today. The test seam stays the CLI process boundary: `tests/native_parity.bats` runs each engine in its own copy of a fixture and compares status, output, and the resulting trees byte for byte; `cargo test` covers each module with values read off the Bash helpers.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, and — proposed in this plan, see decision 2 — sha2 0.11 and signal-hook 0.4 with default features off; dev: assert_cmd 2, predicates 3, tempfile 3. Bash 3.2 for the dispatcher and three Bash fixes, bats-core for conformance. Design: `docs/specs/2026-09-12-rust-migration-design.md`, section "Phase 3 — `sync` transaction and `rollback`". Previous phase: `docs/plans/2026-09-13-rust-migration-phase-2-native-check.md` (closed, receipt 2026-09-13).

## Global Constraints

- `.ai/src/` remains the source of truth. The native `sync` writes only what `lib/sync.sh` writes: resolved tool destinations, `.ai/.sync-manifest`, `.gitignore`, and `.ai/backups/`; `rollback` writes only snapshot targets and `.ai/backups/`; `check` still writes nothing.
- No binary ships to users in this phase: without a built binary every command runs in Bash exactly as today.
- `bin/agentsync.sh` and `lib/**/*.sh` stay Bash 3.2-compatible and clean under `shellcheck -x -S warning -e SC1091`.
- A ported command matches Bash byte for byte on stdout, stderr, exit status, and the files it leaves when stdout is not a terminal, except for the accepted deviations; on a terminal the log's escape codes are those of `lib/helpers/logging.sh`.
- The on-disk formats are written byte-identically: `.ai/.sync-manifest` lines and order, `.ai/backups/<id>/` with `metadata`, `targets.tsv`, `files/`, and `.complete`, the store's `.latest` and `.gitignore`, and the managed `.gitignore` block. A Bash `rollback` restores a backup the binary wrote, and the binary restores one Bash wrote.
- Rust: `unsafe_code = "forbid"`; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean after every task; no YAML or JSON crate. New dependencies are limited to `sha2` 0.11 for manifest digests and `signal-hook` 0.4 for the restore on a signal, both with default features off (decision 2).
- Unit tests that touch the disk are `#[cfg(unix)]`; everything else runs on an in-memory `Workspace` rooted at `/proj`, so `cargo test` stays green on the Windows runner.
- `VERSION` is the only version source; `Cargo.toml` keeps `0.0.0` until Phase 5.
- Accepted deviations already in the spec (Phases 1 and 2) still hold.
- Accepted deviations this plan proposes, recorded in the spec in Task 7's commit and ratified by the plan's review: step lines name the virtual engine and overlay roots; an install-directory `lib/config.yaml` no longer enables post-sync hooks (decision 3); failed filesystem operations report the Rust I/O error; `sync` finishes its transaction when stdout closes; a trapped signal takes effect at the next step; symlinks inside synced trees are copied as their targets; the guard hook's `chmod +x` ignores the process umask; coloured log messages are printed without `echo -e` escape expansion.
- Known quirk reproduced and appended to the spec in Task 7: 12 (`sync --workspace` reports the last failing project's status as "max exit code").
- Commits follow Conventional Commits, scope `native` for engine work, imperative subject, no attribution trailers.

## Decisions for the review

Implementation waits for these. Each has a recommendation, and the tasks are written against it.

Taken on 2026-09-14: all four as recommended. The dependencies were screened against RustSec first — `sha2` has one advisory, RUSTSEC-2021-0100, affecting 0.9.7 only; `signal-hook` and `signal-hook-registry` have none.

1. **Task 0c changes Bash behaviour.** Today a skill directory removed from `.ai/src/` is never pruned from the outputs: `sync_may_prune` looks the directory's path up in a manifest that records only files, so every later sync prints `Kept .claude/skills/<name> (not from .ai/src/ …)`. Rules, which are files, are pruned, and `tests/drift.bats` asserts that. **Recommended:** fix it in Bash first — a directory may be pruned when the manifest records a file below it — so the reference the port mirrors is the intended one. Alternative: reproduce it as known quirk 12 and move the workspace quirk to 13.
2. **Two dependencies.** `sha2` 0.11 (with `digest`, `block-buffer`, `crypto-common`, `hybrid-array`, `typenum`, `cpufeatures`, `cfg-if`, `libc`) replaces `sha256sum`/`shasum`; `signal-hook` 0.4 (with `signal-hook-registry`, `errno`, `libc`) replaces the `trap` handlers, since `unsafe_code = "forbid"` rules out calling `sigaction` directly. The release binary grows from 1.03 MB to 1.28 MB. **Recommended:** both. Alternatives: a hand-written SHA-256 in `src/manifest.rs` (about 80 lines, verified by the same parity fixtures); or no signal handling, in which case a `Ctrl-C` mid-sync leaves a partial write that `agentsync rollback` must undo by hand.
3. **The post-sync trust gate.** Bash enables hooks from `AGENTSYNC_ALLOW_POST_SYNC=true` or from `post_sync.allow: true` in the install directory's `lib/config.yaml`. The binary embeds `lib/config.yaml` and never reads the install directory at runtime (`native-engine.md`). **Recommended:** read the embedded value (`false`) and keep the environment variable; record the deviation; settle where a user-level config lives in Phase 5, when the install directory goes away. Alternative: the dispatcher passes the engine root and the binary reads `<engine>/lib/config.yaml` until Phase 5.
4. **Ratify the eight proposed deviations** in the Global Constraints, worded as Task 7 Step 5 adds them to the spec.

## Module closure

`sync` runs `lib/sync.sh`; `rollback` runs `cmd_rollback` in `bin/agentsync.sh`'s process. The Bash the tasks read, with the lines that matter:

```text
lib/sync.sh                       1-255     entry checks, globals, usage, config path, parse_args
                                  257-307   should_sync_tool, run_post_sync_hook, is_path_protected
                                  668-736   sync_tool and cleanup_tool: counts, skipped names, hooks
                                  740-813   _load_run_config (gitignore.update, outputs, post_sync), version pin
                                  878-1134  --if-stale probe, dest collection, first-sync warning, drift gate,
                                            banner, catalog, backup targets, transaction start
                                  1136-1336 cleanup and signal traps, passes, _finalize_run, main
bin/agentsync.sh                  185-246   cmd_workspace_fanout
                                  280-352   _NATIVE_COMMANDS, _native_try, dispatch
lib/helpers/manifest.sh           19-295    hash, load, drift, record, write
lib/helpers/backup.sh             18-680    validation, create, restore, latest, list, prune
                                  682-859   rollback traps, usage, cmd_rollback
lib/helpers/gitignore.sh          1-90      managed block
lib/helpers/file_ops.sh           17-199    dry-run branches, sync_may_prune, sync_note_preserved
lib/helpers/rule_operations.sh    180-527   dry-run and preserve branches of merge, sync_rules, command skills
lib/helpers/format_conversion.sh  179-515   per-file dry-run lines, _sweep_generated
lib/helpers/opencode.sh           475-513   sync_opencode_config dry-run
lib/helpers/shared.sh             149-221   shared_setup_overlay
lib/helpers/paths.sh              336-387   ai_dir_enclosing_root, find_workspace_ai_dirs
lib/helpers/prompts.sh            10-37     is_tty, prompt_confirm
lib/helpers/tmp.sh                100-127   tmp_sibling: mktemp mode, cp -p of an existing mode
lib/helpers/logging.sh            12-101    the coloured voice, log_done
```

Out of this phase, as the spec orders: every other command stays Bash, including `init`, which runs `lib/sync.sh` directly for its first sync.

---

### Task 0: Baseline

**Files:**
- None changed.

**Interfaces:**
- Consumes: branch `feat/native-engine-phase-1` with Phase 2 closed; `cargo` on `PATH` (or `~/.cargo/bin/cargo`).
- Produces: a recorded green baseline to count against.

- [x] **Step 1: Confirm the branch and the toolchain**

```bash
git branch --show-current
git status --short
cargo --version
bats --version
```

Expected: `feat/native-engine-phase-1`; empty status; `cargo 1.85` or newer; `Bats 1.5` or newer (the parity helpers use `$BATS_TEST_TMPDIR`).

- [x] **Step 2: Record the baseline**

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release
bats --jobs 4 tests/ --tap | head -1
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
```

Expected: `112 passed` (unit) and `7 passed` (integration); the TAP plan is `1..762`; `0` native failures; ShellCheck exits 0.

---

### Task 0b: Prefactor — A Path Several Tools Write Is Counted Once

**Files:**
- Modify: `lib/sync.sh:958-963` (`_warn_baseline_replacements`)
- Modify: `tests/baseline.bats` (one regression test)

**Interfaces:**
- Consumes: nothing new.
- Produces: the first-sync warning counts the distinct paths it lists. Found while writing the `sync` fixtures: with `cursor` and `codex` enabled and a hand-written `AGENTS.md`, Bash prints `regenerating 4 path(s) that already exist:` above a single line, `AGENTS.md`, because `${#existing[@]}` counts one entry per tool that writes the path before `sort -u` removes the duplicates.

- [x] **Step 1: Write the failing test in `tests/baseline.bats`, before `@test "baseline: an empty generated directory is not reported"`**

```bash
@test "baseline: a path several tools write is counted once" {
    enable_tools cursor codex
    printf '# Hand-written agents\n' > AGENTS.md
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"regenerating 1 path(s) that already exist"* ]]
}

```

- [x] **Step 2: Run it, confirm it fails**

Run: `bats tests/baseline.bats -f 'counted once'`
Expected: `not ok 1 baseline: a path several tools write is counted once`, failing at the `regenerating 1 path(s)` assertion.

- [x] **Step 3: Count after deduplication in `lib/sync.sh`**

Replace:

```bash
    [[ ${#existing[@]} -gt 0 ]] || return 0

    log_warning "First sync in this project — regenerating ${#existing[@]} path(s) that already exist:"
    while IFS= read -r rel; do
        echo "      $rel" >&2
    done < <(printf '%s\n' "${existing[@]}" | LC_ALL=C sort -u)
```

with:

```bash
    [[ ${#existing[@]} -gt 0 ]] || return 0

    local -a unique=()
    while IFS= read -r rel; do
        unique+=("$rel")
    done < <(printf '%s\n' "${existing[@]}" | LC_ALL=C sort -u)

    log_warning "First sync in this project — regenerating ${#unique[@]} path(s) that already exist:"
    for rel in "${unique[@]}"; do
        echo "      $rel" >&2
    done
```

- [x] **Step 4: Run the file and ShellCheck, confirm green**

```bash
bats tests/baseline.bats
shellcheck -x -S warning -e SC1091 lib/sync.sh
```

Expected: `1..11`, 11 ok; ShellCheck exits 0.

- [x] **Step 5: Commit**

```bash
git add lib/sync.sh tests/baseline.bats docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "fix(sync): count each replaced path once in the first-sync warning"
```

---

### Task 0c: Prefactor — A Generated Skill Directory Is Pruned

**Files:**
- Modify: `lib/helpers/file_ops.sh:81-91` (`sync_may_prune`)
- Modify: `lib/helpers/manifest.sh` (new `manifest_has_entry_below`, above `manifest_load`)
- Modify: `tests/drift.bats` (two tests)

**Interfaces:**
- Consumes: nothing new.
- Produces: `manifest_has_entry_below <rel-dir>` returns 0 when the loaded manifest records a file below the directory; `sync_may_prune` allows pruning such a directory. **Decision 1 of the review.** The second test pins the unchanged half: a directory the manifest records nothing below stays and is reported as kept.

- [x] **Step 1: Write the tests in `tests/drift.bats`, after `@test "drift: obsolete sync-generated rule is still pruned when removed from source"`**

```bash

@test "drift: obsolete sync-generated skill directory is pruned when removed from source" {
    mkdir -p .ai/src/skills/temp-skill
    printf -- '---\nname: temp-skill\n---\n' > .ai/src/skills/temp-skill/SKILL.md
    AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync >/dev/null
    [ -f ".claude/skills/temp-skill/SKILL.md" ]
    rm -rf .ai/src/skills/temp-skill
    run env AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync
    [ "$status" -eq 0 ]
    [[ "$output" != *"Kept .claude/skills/temp-skill"* ]]
    [ ! -e ".claude/skills/temp-skill" ]
}

@test "drift: sync preserves a user-added skill directory in a generated dir" {
    mkdir -p .claude/skills/my-own
    echo "mine" > .claude/skills/my-own/SKILL.md
    run env AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"Kept .claude/skills/my-own"* ]]
    [ -f ".claude/skills/my-own/SKILL.md" ]
}
```

- [x] **Step 2: Run them, confirm the first fails**

Run: `bats tests/drift.bats -f 'skill directory'`
Expected: `not ok 1 drift: obsolete sync-generated skill directory is pruned when removed from source`; `ok 2 drift: sync preserves a user-added skill directory in a generated dir`.

- [x] **Step 3: Add `manifest_has_entry_below` to `lib/helpers/manifest.sh`, directly above the `# Load existing manifest from disk into MANIFEST_KEYS/VALUES.` comment**

```bash
# True (0) when the loaded manifest records a file below directory <rel>.
# Usage: manifest_has_entry_below "rel/dir"
manifest_has_entry_below() {
    local prefix="$1/"
    local key
    for key in "${MANIFEST_KEYS[@]+"${MANIFEST_KEYS[@]}"}"; do
        [[ "$key" == "$prefix"* ]] && return 0
    done
    return 1
}

```

- [x] **Step 4: Consult it in `sync_may_prune` in `lib/helpers/file_ops.sh`**

Replace:

```bash
    manifest_lookup "$rel" >/dev/null 2>&1 && return 0
    return 1
}
```

with:

```bash
    manifest_lookup "$rel" >/dev/null 2>&1 && return 0
    # The manifest records files, so a directory sync generated shows up only
    # through the entries below it.
    if [[ -d "$dest_path" ]] && declare -f manifest_has_entry_below >/dev/null 2>&1; then
        manifest_has_entry_below "$rel" && return 0
    fi
    return 1
}
```

- [x] **Step 5: Run the affected files and ShellCheck, confirm green**

```bash
bats --jobs 4 tests/drift.bats tests/files.bats tests/base_skills.bats tests/shared.bats tests/profiles.bats tests/sync.bats tests/adopt.bats tests/doctor.bats --tap | grep -c '^not ok'
bats tests/drift.bats | head -1
shellcheck -x -S warning -e SC1091 lib/helpers/file_ops.sh lib/helpers/manifest.sh
```

Expected: `0`; `1..28`; ShellCheck exits 0.

- [x] **Step 6: Commit**

```bash
git add lib/helpers/file_ops.sh lib/helpers/manifest.sh tests/drift.bats docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "fix(sync): prune a generated directory the manifest records files under"
```

---

### Task 0d: Prefactor — The `.gitignore` Block Sorts by Bytes

**Files:**
- Modify: `lib/helpers/gitignore.sh:36` (`update_gitignore`)
- Modify: `tests/gitignore.bats` (one regression test)

**Interfaces:**
- Consumes: nothing new.
- Produces: the managed block is sorted with `LC_ALL=C sort -u`. Found while porting it: `sort | uniq` follows the locale, so the committed `.gitignore` block orders `B/`, `_x/`, `b/` differently on macOS, glibc `en_US.UTF-8`, and `C` — the platform-dependent ordering the architecture rules forbid, and the parity reference must not depend on it (skill `native-port`, triage rule 1). The test skips where `en_US.UTF-8` is not installed, because only a collating locale can show the difference.

- [x] **Step 1: Write the failing test in `tests/gitignore.bats`, before `@test "update_gitignore handles empty paths"`**

```bash
@test "update_gitignore orders paths by bytes whatever the locale" {
    locale -a 2>/dev/null | grep -qix 'en_US.utf-\{0,1\}8' || skip "en_US.UTF-8 locale not installed"
    LC_ALL=en_US.UTF-8 update_gitignore "$TEST_PROJECT/.gitignore" "$(printf '%s\n' b/ _x/ B/)"
    run grep -A3 "Do not edit this block manually" "$TEST_PROJECT/.gitignore"
    [ "${lines[1]}" = "B/" ]
    [ "${lines[2]}" = "_x/" ]
    [ "${lines[3]}" = "b/" ]
}

```

- [x] **Step 2: Run it, confirm it fails**

Run: `bats tests/gitignore.bats -f 'by bytes'`
Expected on macOS or a glibc host with `en_US.UTF-8`: `not ok 1 update_gitignore orders paths by bytes whatever the locale`. Where the locale is missing the test reports `skip`; run this step on a host that has it.

- [x] **Step 3: Sort bytewise in `lib/helpers/gitignore.sh`**

Replace:

```bash
        sorted_paths=$(printf '%s\n' "$paths_string" | sed '/^$/d' | sort | uniq)
```

with:

```bash
        sorted_paths=$(printf '%s\n' "$paths_string" | sed '/^$/d' | LC_ALL=C sort -u)
```

- [x] **Step 4: Run the file and ShellCheck, confirm green**

```bash
bats tests/gitignore.bats
shellcheck -x -S warning -e SC1091 lib/helpers/gitignore.sh
```

Expected: `1..7`, 7 ok; ShellCheck exits 0.

- [x] **Step 5: Commit**

```bash
git add lib/helpers/gitignore.sh tests/gitignore.bats docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "fix(sync): sort the .gitignore block by bytes in every locale"
```

---

### Task 1: Workspace on Disk, Containment Through the Disk, and a Streaming Log

**Files:**
- Modify: `src/workspace.rs` (whole file)
- Modify: `src/log.rs` (whole file)
- Modify: `src/paths.rs` (`Paths` gains `lexical_below_root` and `on_disk`; one test)
- Modify: `src/overlay.rs`, `src/file_ops.rs`, `src/rules.rs`, `src/render.rs`, `src/cli/check.rs` (the new `Result` of `remove` and `create_dir_all`)

**Interfaces:**
- Consumes: Phase 2's `Workspace`, `Paths`, `Log`.
- Produces: `Workspace::on_disk(root: &str) -> Workspace` — paths below `root` read and write the disk, `/<agentsync>` and `/<agentsync-overlay>` stay in memory; `Workspace::create_dir_all(&mut self, &str) -> Result<(), Error>` and `Workspace::remove(&mut self, &str) -> Result<(), Error>` (both were infallible); `Workspace::copy` of a disk file onto the disk goes through `std::fs::copy`, so modes travel as `cp` carries them. `Paths::on_disk(root: &str) -> Paths` canonicalises below-root paths through the disk, where `Paths::for_disk_root` stays lexical for `check`. `log::Sink = Box<dyn FnMut(Stream, &str)>`; `Log::streaming(colors: bool, sink: Sink) -> Log` hands every line to the sink instead of keeping it; `Log::done(&mut self, &str)` is `log_done`; with `colors` the tagged lines carry `logging.sh`'s escape codes and emoji. `overlay::cleanup_profile(&mut Workspace) -> Result<(), Error>` and `overlay::merge_shared_parent(…) -> Result<(), Error>`.

- [x] **Step 1: Replace `src/workspace.rs`**

```rust
//! The file tree a render reads and writes.
//!
//! `lib/check.sh` copied `.ai/` and the manifest's outputs into a temporary
//! root with `tar` and ran `sync.sh` there. The in-memory workspace is that copy
//! without the copy: paths below the project root and below the virtual engine
//! and overlay roots are served from an index of disk paths, embedded templates,
//! and bytes written by the render. A workspace on disk reads and writes the
//! project itself, as `sync.sh` does, and keeps only the virtual roots in memory.

use std::collections::BTreeMap;
use std::io::Write;
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
    on_disk: bool,
    entries: BTreeMap<String, Entry>,
}

impl Workspace {
    /// An empty in-memory tree rooted at `root`, with the embedded engine mounted.
    pub fn new(root: &str) -> Self {
        let mut ws = Self {
            root: root.to_string(),
            on_disk: false,
            entries: BTreeMap::new(),
        };
        ws.mkdir_entries(root);
        for (rel, bytes) in catalog::engine_files() {
            ws.insert_file(&format!("{ENGINE_ROOT}/{rel}"), Content::Embedded(bytes));
        }
        ws
    }

    /// The project at `root` read and written on disk; the embedded engine and
    /// the overlay trees stay in memory.
    pub fn on_disk(root: &str) -> Self {
        let mut ws = Self::new(root);
        ws.entries.retain(|path, _| paths::is_virtual(path));
        ws.on_disk = true;
        ws
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    fn indexed(&self, path: &str) -> bool {
        paths::is_virtual(path) || (!self.on_disk && paths::is_within(path, &self.root))
    }

    fn writes_disk(&self, path: &str) -> bool {
        self.on_disk && !paths::is_virtual(path)
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
        self.mkdir_entries(at);
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

    /// Adds a file to the in-memory index, creating its parents.
    pub fn insert_file(&mut self, path: &str, content: Content) {
        self.mkdir_entries(&paths::parent(path));
        self.entries.insert(path.to_string(), Entry::File(content));
    }

    fn mkdir_entries(&mut self, path: &str) {
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

    /// `mkdir -p`.
    pub fn create_dir_all(&mut self, path: &str) -> Result<(), Error> {
        if self.writes_disk(path) {
            return std::fs::create_dir_all(path).map_err(|e| Error::io(path, e));
        }
        self.mkdir_entries(path);
        Ok(())
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
        if self.writes_disk(path) {
            return std::fs::write(path, bytes).map_err(|e| Error::io(path, e));
        }
        if !self.is_dir(&paths::parent(path)) || self.is_dir(path) {
            return Err(not_found(path));
        }
        self.entries
            .insert(path.to_string(), Entry::File(Content::Bytes(bytes)));
        Ok(())
    }

    /// `>>`: creates the file when missing, the parent directory must exist.
    pub fn append(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.writes_disk(path) {
            return std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| file.write_all(bytes))
                .map_err(|e| Error::io(path, e));
        }
        let mut current = if self.is_file(path) {
            self.read(path)?
        } else {
            Vec::new()
        };
        current.extend_from_slice(bytes);
        self.write(path, current)
    }

    /// `rm -rf`.
    pub fn remove(&mut self, path: &str) -> Result<(), Error> {
        if self.writes_disk(path) {
            let removed = match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
                Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
                Ok(_) => std::fs::remove_file(path),
            };
            return removed.map_err(|e| Error::io(path, e));
        }
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
        Ok(())
    }

    /// `cp -r src dst` onto a missing `dst`: a file, or a whole tree with its
    /// empty directories.
    pub fn copy(&mut self, src: &str, dst: &str) -> Result<(), Error> {
        if self.is_file(src) {
            let content = self.content_of(src)?;
            if self.writes_disk(dst) {
                return match content {
                    Content::Disk(disk) => std::fs::copy(&disk, dst).map(|_| ()),
                    Content::Embedded(bytes) => std::fs::write(dst, bytes),
                    Content::Bytes(bytes) => std::fs::write(dst, bytes),
                }
                .map_err(|e| Error::io(dst, e));
            }
            if !self.is_dir(&paths::parent(dst)) {
                return Err(not_found(dst));
            }
            self.entries.insert(dst.to_string(), Entry::File(content));
            return Ok(());
        }
        if !self.is_dir(src) {
            return Err(not_found(src));
        }
        self.create_dir_all(dst)?;
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
        ws.create_dir_all("/proj/.ai/src/skills/empty").unwrap();
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
        ws.create_dir_all("/proj/out").unwrap();
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
        ws.remove("/proj/copy/rules").unwrap();
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

    #[cfg(unix)]
    #[test]
    fn a_workspace_on_disk_writes_the_project_and_keeps_the_engine_in_memory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.path().join(".ai/src/skills/a/scripts")).unwrap();
        let script = dir.path().join(".ai/src/skills/a/scripts/run.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut ws = Workspace::on_disk(&root);
        assert!(ws.is_file("/<agentsync>/lib/config.yaml"));
        assert!(!ws.exists(&format!("{root}/CLAUDE.md")));

        ws.copy(
            "/<agentsync>/lib/templates/settings/claude.json",
            &format!("{root}/settings.json"),
        )
        .unwrap();
        assert!(dir.path().join("settings.json").is_file());

        ws.copy(
            &format!("{root}/.ai/src/skills/a"),
            &format!("{root}/.claude/skills/a"),
        )
        .unwrap();
        let copied = dir.path().join(".claude/skills/a/scripts/run.sh");
        assert_eq!(
            std::fs::metadata(&copied).unwrap().permissions().mode() & 0o777,
            0o755
        );

        ws.append(&format!("{root}/.claude/skills/a/x.md"), b"1\n")
            .unwrap();
        ws.append(&format!("{root}/.claude/skills/a/x.md"), b"2\n")
            .unwrap();
        assert_eq!(
            ws.read(&format!("{root}/.claude/skills/a/x.md")).unwrap(),
            b"1\n2\n"
        );

        ws.remove(&format!("{root}/.claude")).unwrap();
        ws.remove(&format!("{root}/.claude")).unwrap();
        assert!(!dir.path().join(".claude").exists());
        assert!(ws.write(&format!("{root}/missing/x"), Vec::new()).is_err());
    }
}
```

- [x] **Step 2: Replace `src/log.rs`**

```rust
//! The engine's log voice, mirroring `lib/helpers/logging.sh`. `check` captures
//! the render log plain, the way `lib/check.sh` captured `sync.sh` into a file
//! where `_use_colors` is false; `sync` streams it, coloured on a terminal.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// Receives each line as it is written, without its newline.
pub type Sink = Box<dyn FnMut(Stream, &str)>;

#[derive(Default)]
pub struct Log {
    lines: Vec<(Stream, String)>,
    sink: Option<Sink>,
    colors: bool,
}

pub const SEPARATOR: &str = "═══════════════════════════════════════════════════════════════";

const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[0;34m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const RED: &str = "\x1b[0;31m";

impl Log {
    /// A log that hands every line to `sink` instead of keeping it; `colors` is
    /// `_use_colors`, decided by the caller from stdout and `NO_COLOR`.
    pub fn streaming(colors: bool, sink: Sink) -> Self {
        Self {
            lines: Vec::new(),
            sink: Some(sink),
            colors,
        }
    }

    fn tagged(&mut self, stream: Stream, color: &str, emoji: &str, tag: &str, msg: &str) {
        let line = if self.colors {
            format!("{color}{emoji} {tag}{RESET} {msg}")
        } else {
            format!("{tag} {msg}")
        };
        self.emit(stream, line);
    }

    pub fn info(&mut self, msg: &str) {
        self.tagged(Stream::Out, BLUE, "🔵", "[INFO]", msg);
    }

    pub fn success(&mut self, msg: &str) {
        self.tagged(Stream::Out, GREEN, "✅", "[SUCCESS]", msg);
    }

    pub fn warning(&mut self, msg: &str) {
        self.tagged(Stream::Out, YELLOW, "⚠\u{fe0f} ", "[WARNING]", msg);
    }

    pub fn error(&mut self, msg: &str) {
        self.tagged(Stream::Err, RED, "❌", "[ERROR]", msg);
    }

    pub fn done(&mut self, msg: &str) {
        self.tagged(Stream::Out, GREEN, "✅", "[DONE]", msg);
    }

    pub fn step(&mut self, msg: &str) {
        self.out(format!("   📁 {msg}"));
    }

    pub fn separator(&mut self) {
        self.out(SEPARATOR.to_string());
    }

    pub fn out(&mut self, line: String) {
        self.emit(Stream::Out, line);
    }

    pub fn err(&mut self, line: String) {
        self.emit(Stream::Err, line);
    }

    fn emit(&mut self, stream: Stream, line: String) {
        match &mut self.sink {
            Some(sink) => sink(stream, &line),
            None => self.lines.push((stream, line)),
        }
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
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn plain_prefixes_match_logging_sh_without_a_terminal() {
        let mut log = Log::default();
        log.info("a");
        log.warning("b");
        log.error("c");
        log.step("d");
        log.success("e");
        log.done("f");
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[INFO] a",
                "[WARNING] b",
                "[ERROR] c",
                "   📁 d",
                "[SUCCESS] e",
                "[DONE] f"
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

    #[test]
    fn a_streaming_log_hands_coloured_lines_to_its_sink_in_order() {
        let seen: Rc<RefCell<Vec<(Stream, String)>>> = Rc::default();
        let sink_seen = Rc::clone(&seen);
        let mut log = Log::streaming(
            true,
            Box::new(move |stream, line| sink_seen.borrow_mut().push((stream, line.to_string()))),
        );
        log.info("Syncing Claude Code...");
        log.warning("w");
        log.error("e");
        log.done("Synced 1/1 tools");
        log.step("s");
        assert!(log.lines().is_empty());
        assert_eq!(
            *seen.borrow(),
            [
                (
                    Stream::Out,
                    "\x1b[0;34m🔵 [INFO]\x1b[0m Syncing Claude Code...".to_string()
                ),
                (
                    Stream::Out,
                    "\x1b[0;33m⚠\u{fe0f}  [WARNING]\x1b[0m w".to_string()
                ),
                (Stream::Err, "\x1b[0;31m❌ [ERROR]\x1b[0m e".to_string()),
                (
                    Stream::Out,
                    "\x1b[0;32m✅ [DONE]\x1b[0m Synced 1/1 tools".to_string()
                ),
                (Stream::Out, "   📁 s".to_string()),
            ]
        );
    }
}
```

- [x] **Step 3: Resolve below-root paths through the disk in `src/paths.rs`**

Replace the `Paths` struct, `Paths::new`, and `Paths::for_disk_root`:

```rust
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
```

with:

```rust
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: String,
    pub root_canonical: String,
    home: Option<String>,
    lexical_below_root: bool,
}

impl Paths {
    pub fn new(root: &str, root_canonical: &str, home: Option<&str>) -> Self {
        Self {
            root: root.to_string(),
            root_canonical: root_canonical.to_string(),
            home: home.filter(|h| !h.is_empty()).map(str::to_string),
            lexical_below_root: true,
        }
    }

    /// Paths for a root on disk, canonicalised the way `cd -P && pwd` does.
    pub fn for_disk_root(root: &str) -> Self {
        let canonical = std::fs::canonicalize(root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| root.to_string());
        Self::new(root, &canonical, std::env::var("HOME").ok().as_deref())
    }

    /// Paths for a project a render writes in place: below the root, too, a
    /// path is canonicalised through its nearest existing ancestor on disk, so
    /// a symlinked directory cannot carry a destination out of the project.
    pub fn on_disk(root: &str) -> Self {
        Self {
            lexical_below_root: false,
            ..Self::for_disk_root(root)
        }
    }
```

Then replace the head of `canonicalize_with_existing_ancestor`:

```rust
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
```

with:

```rust
    /// `canonicalize_with_existing_ancestor_r`. Below the root of an in-memory
    /// render the result is lexical: `check` renders into a workspace that, like
    /// the tar copy Bash rendered into, has no symlinks under the root. Virtual
    /// roots are their own canonical form; anything else resolves through the disk.
    pub fn canonicalize_with_existing_ancestor(&self, abs: &str) -> Option<String> {
        if is_virtual(abs) {
            return Some(abs.to_string());
        }
        if let Some(rest) = abs.strip_prefix(&self.root)
            && self.lexical_below_root
            && (rest.is_empty() || rest.starts_with('/'))
        {
```

Add this test in the `tests` module, before `fn the_logical_root_prefers_pwd_when_it_names_the_same_directory`:

```rust
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_below_the_root_cannot_carry_a_dest_outside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), root.join(".claude")).unwrap();
        let root = root.to_string_lossy().into_owned();
        let mut log = Log::default();

        let lexical = Paths::for_disk_root(&root);
        assert!(
            lexical
                .resolve_dest(
                    ".claude/rules",
                    "targets.rules.dest for Claude Code",
                    &mut log
                )
                .is_some()
        );

        let on_disk = Paths::on_disk(&root);
        assert_eq!(
            on_disk.resolve_dest(
                ".claude/rules",
                "targets.rules.dest for Claude Code",
                &mut log
            ),
            None
        );
        let outside = std::fs::canonicalize(dir.path().join("outside")).unwrap();
        assert_eq!(
            log.tail(1),
            [format!(
                "[ERROR] targets.rules.dest for Claude Code resolves outside repository root: .claude/rules -> {}/rules",
                outside.to_string_lossy()
            )]
        );
        assert_eq!(
            on_disk.resolve_dest("CLAUDE.md", "targets.agents.dest for Claude Code", &mut log),
            Some(format!("{root}/CLAUDE.md"))
        );
    }

```

- [x] **Step 4: Propagate the fallible `remove` and `create_dir_all`**

In `src/overlay.rs`, `build_tree` gains `?` on its three calls:

```rust
    ws.remove(&dir)?;
    let src = format!("{dir}/src");
    ws.create_dir_all(&src)?;
```

```rust
            ws.create_dir_all(&paths::parent(&target))?;
```

`cleanup_profile` returns the result:

```rust
pub fn cleanup_profile(ws: &mut Workspace) -> Result<(), Error> {
    ws.remove(&format!("{OVERLAY_ROOT}/profile"))
}
```

`merge_shared_parent` becomes fallible — replace its signature line and first statements, and end the function with `Ok(())`:

```rust
pub fn merge_shared_parent(
    ws: &mut Workspace,
    parent_src: &str,
    categories: &[&str],
) -> Result<(), Error> {
    let child_src = format!("{}/.ai/src", ws.root());
    ws.create_dir_all(&child_src)?;
```

```rust
            }
        }
    }
    Ok(())
}
```

and its test calls `merge_shared_parent(&mut ws, &parent, &["rules"]).unwrap();`.

In `src/cli/check.rs`, `merge_shared_parent` propagates:

```rust
        overlay::merge_shared_parent(ws, &parent, &overlay::inherit_categories(&inherit))?;
```

In `src/file_ops.rs`, `cleanup_path` ignores the result and the other calls propagate — the six statements become, in file order:

```rust
    let _ = s.ws.remove(target);
```

```rust
    s.ws.create_dir_all(&paths::parent(dest))?;
```

```rust
        s.ws.remove(dest)?;
```

```rust
    s.ws.create_dir_all(dest)?;
```

```rust
        s.ws.remove(&target)?;
```

```rust
        s.ws.remove(&item)?;
```

In `src/rules.rs`, every `s.ws.create_dir_all(…);` and `s.ws.remove(…);` statement gains `?`; in file order they read:

```rust
    s.ws.create_dir_all(&paths::parent(dest_file))?;
    s.ws.remove(dest_file)?;
```

```rust
    s.ws.create_dir_all(dest_dir)?;
```

```rust
        s.ws.remove(&path)?;
```

```rust
    s.ws.create_dir_all(dest_dir)?;
```

```rust
        s.ws.create_dir_all(&skill_dir)?;
```

```rust
            s.ws.create_dir_all(&format!("{skill_dir}/agents"))?;
```

```rust
            s.ws.remove(&policy)?;
            let agents_dir = format!("{skill_dir}/agents");
            if s.ws.list(&agents_dir).is_empty() {
                s.ws.remove(&agents_dir)?;
            }
```

```rust
            s.ws.remove(&format!("{dest_dir}/{name}"))?;
```

```rust
        s.ws.create_dir_all(dest_dir)?;
```

```rust
            s.ws.remove(&path)?;
```

In `src/render.rs`, the two sites map the error to the run's stop:

```rust
        overlay::cleanup_profile(&mut s.ws).map_err(|e| io(s, e))?;
```

```rust
        Ok(composed) => {
            s.ws.create_dir_all(&paths::parent(dest))
                .map_err(|e| io(s, e))?;
            s.ws.remove(dest).map_err(|e| io(s, e))?;
```

- [x] **Step 5: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: `115 passed` (unit) and `7 passed` (integration); fmt and clippy exit 0. The three new tests are `a_workspace_on_disk_writes_the_project_and_keeps_the_engine_in_memory` (a `0755` script keeps its mode through `copy`), `a_streaming_log_hands_coloured_lines_to_its_sink_in_order`, and `a_symlinked_directory_below_the_root_cannot_carry_a_dest_outside`.

- [x] **Step 6: Confirm the coloured prefixes against `logging.sh`**

```bash
script -q /dev/null bash -c 'source lib/helpers/logging.sh; log_info a; log_warning b; log_done c' | od -c | head -8
```

Expected: `033 [ 0 ; 3 4 m` before `🔵 [INFO]`, `033 [ 0 ; 3 3 m` before `⚠️  [WARNING]` (two spaces), `033 [ 0 ; 3 2 m` before `✅ [DONE]`, each tag closed by `033 [ 0 m` and followed by a space — the bytes the streaming log test asserts. This is the macOS form of `script`; on Linux run `script -qc '<command>' /dev/null`. `script` needs a pseudo-terminal; a sandbox that denies `openpty` must run this step outside it.

- [x] **Step 7: Commit**

```bash
git add src/workspace.rs src/log.rs src/paths.rs src/overlay.rs src/file_ops.rs src/rules.rs src/render.rs src/cli/check.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): write the project through the workspace and stream the log"
```

---

### Task 2: Dry Run and Untracked Outputs in Every Render Step

**Files:**
- Modify: `src/session.rs` (whole file)
- Modify: `src/file_ops.rs` (whole file)
- Modify: `src/rules.rs` (module doc, `merge_rules_to_file`, `sync_rules`, `sync_commands_as_skills`, `Conversion::dry_run_label`, `sync_converted`, one test)
- Modify: `src/render.rs` (the render's own dry-run branches, the guard's execute bit)
- Modify: `src/workspace.rs` (`make_executable`)

**Interfaces:**
- Consumes: `Workspace::on_disk`, fallible `remove`/`create_dir_all` (Task 1); Task 0c's pruning rule.
- Produces: `Session::dry_run: bool` and `Session::force: bool` (public fields, both `false` by default, which is what `check` needs); `Session::activate_manifest(&mut self, paths: BTreeSet<String>)` (`SYNC_MANIFEST_ACTIVE` with `MANIFEST_KEYS`); `Session::may_prune(&self, abs: &str) -> bool` (`sync_may_prune` with Task 0c's directory rule); `Session::note_preserved(&mut self, shown: &str)` and `Session::preserved(&self) -> usize` (`sync_note_preserved`, `SYNC_PRESERVED_COUNT`); `Workspace::make_executable(&mut self, path: &str) -> Result<(), Error>`. Every copy, sweep, converter, inliner, and composer logs its `(dry-run)` or `Would …` line and writes nothing under `dry_run`, and keeps an extraneous entry `may_prune` refuses.

- [x] **Step 1: Replace `src/session.rs`**

```rust
//! State one render shares across its steps: the workspace, path rules, the
//! log, the run's `--dry-run` and `--force`, and the manifest's record of what
//! this run wrote (`manifest.sh`).

use std::collections::BTreeSet;

use crate::log::Log;
use crate::paths::Paths;
use crate::workspace::Workspace;

pub struct Session {
    pub ws: Workspace,
    pub paths: Paths,
    pub log: Log,
    pub dry_run: bool,
    pub force: bool,
    manifest: Option<BTreeSet<String>>,
    preserved: usize,
    touched: BTreeSet<String>,
    legacy_payload_warned: bool,
}

impl Session {
    pub fn new(ws: Workspace, paths: Paths) -> Self {
        Self {
            ws,
            paths,
            log: Log::default(),
            dry_run: false,
            force: false,
            manifest: None,
            preserved: 0,
            touched: BTreeSet::new(),
            legacy_payload_warned: false,
        }
    }

    pub fn display(&self, path: &str) -> String {
        self.paths.display(path)
    }

    /// `SYNC_MANIFEST_ACTIVE="true"` with `MANIFEST_KEYS` loaded: from here on
    /// a sweep keeps what the previous sync did not generate.
    pub fn activate_manifest(&mut self, paths: BTreeSet<String>) {
        self.manifest = Some(paths);
    }

    /// `sync_may_prune`: outside a manifest-aware run, or under `--force`,
    /// every extraneous entry may go; otherwise a file the manifest records, or
    /// a directory it records a file below.
    pub fn may_prune(&self, abs: &str) -> bool {
        let Some(manifest) = &self.manifest else {
            return true;
        };
        if self.force {
            return true;
        }
        self.paths.to_repo_relative(abs).is_none_or(|rel| {
            let below = format!("{rel}/");
            manifest.contains(&rel)
                || (self.ws.is_dir(abs)
                    && manifest
                        .range(below.clone()..)
                        .next()
                        .is_some_and(|key| key.starts_with(&below)))
        })
    }

    /// `sync_note_preserved`.
    pub fn note_preserved(&mut self, shown: &str) {
        if self.dry_run {
            self.log.warning(&format!(
                "Would keep {shown} (not from .ai/src/; --force to prune)"
            ));
        } else {
            self.log.warning(&format!(
                "Kept {shown} (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)"
            ));
        }
        self.preserved += 1;
    }

    pub fn preserved(&self) -> usize {
        self.preserved
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

    #[test]
    fn only_manifest_paths_may_be_pruned_once_the_manifest_is_active_unless_forced() {
        let mut s = test_session();
        s.ws.create_dir_all("/proj/.claude/skills/old").unwrap();
        s.ws.create_dir_all("/proj/.claude/skills/mine").unwrap();
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
        s.activate_manifest(BTreeSet::from([
            ".claude/rules/old.md".to_string(),
            ".claude/skills/old/SKILL.md".to_string(),
        ]));
        assert!(s.may_prune("/proj/.claude/rules/old.md"));
        assert!(!s.may_prune("/proj/.claude/rules/mine.md"));
        assert!(s.may_prune("/proj/.claude/skills/old"));
        assert!(!s.may_prune("/proj/.claude/skills/mine"));
        assert!(!s.may_prune("/proj/.claude/skills/ol"));
        assert!(s.may_prune("/elsewhere/mine.md"));
        s.force = true;
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
    }

    #[test]
    fn a_preserved_entry_is_counted_and_worded_for_the_run() {
        let mut s = test_session();
        s.note_preserved(".claude/rules/mine.md");
        s.dry_run = true;
        s.note_preserved(".claude/rules/other.md");
        assert_eq!(s.preserved(), 2);
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] Kept .claude/rules/mine.md (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "[WARNING] Would keep .claude/rules/other.md (not from .ai/src/; --force to prune)"
            ]
        );
    }
}
```

- [x] **Step 2: Replace `src/file_ops.rs`**

```rust
//! `lib/helpers/file_ops.sh`: copies and sweeps that honour `--dry-run`, and
//! prune an extraneous entry only when `Session::may_prune` allows it.

use crate::session::Session;
use crate::{Error, filters, paths};

/// `cleanup_path`: removes `target` when it exists; true when something went.
/// A failed removal is not an error: Bash calls it inside an `if`, where
/// `set -e` does not apply.
pub fn cleanup_path(s: &mut Session, target: &str) -> bool {
    if !s.ws.exists(target) {
        return false;
    }
    let shown = s.display(target);
    if s.dry_run {
        s.log.step(&format!("Would remove: {shown} (dry-run)"));
    } else {
        let _ = s.ws.remove(target);
        s.log.step(&format!("Removed: {shown}"));
    }
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
    if s.dry_run {
        s.log.step(&format!("{src_disp} → {dest_disp} (dry-run)"));
        return Ok(());
    }
    s.ws.create_dir_all(&paths::parent(dest))?;
    if s.ws.is_dir(dest) {
        s.ws.copy(src, &format!("{dest}/{}", paths::leaf(src)))?;
    } else {
        let _ = s.ws.remove(dest);
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
    if !s.dry_run {
        s.ws.create_dir_all(dest)?;
    }

    let mut source_items: Vec<String> = Vec::new();
    for name in s.ws.glob(src) {
        if !filters::matches(&name, include, exclude) {
            continue;
        }
        source_items.push(name.clone());
        if s.dry_run {
            continue;
        }
        let target = format!("{dest}/{name}");
        let _ = s.ws.remove(&target);
        s.ws.copy(&format!("{src}/{name}"), &target)?;
        if s.ws.is_dir(&target) {
            s.record_tree(&target);
        } else {
            s.record_write(&target);
        }
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
        if !s.may_prune(&item) {
            s.note_preserved(&format!("{dest_disp}/{name}"));
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove: {dest_disp}/{name} (extraneous)"));
        } else {
            s.ws.remove(&item)?;
            s.log.step(&format!("Removed: {dest_disp}/{name}"));
        }
        cleaned += 1;
    }

    let extra = if include.is_empty() {
        String::new()
    } else {
        format!(", include='{include}'")
    };
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ ({} updates, {cleaned} cleanups){extra}{suffix}",
        source_items.len()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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

    #[test]
    fn a_dry_run_reports_every_change_and_makes_none() {
        let mut s = test_session();
        s.dry_run = true;
        file(&mut s, "/proj/.ai/src/AGENTS.md", "new");
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.claude/skills/stale/SKILL.md", "s");
        file(&mut s, "/proj/.cursor/rules/core.mdc", "x");
        copy_file(&mut s, "/proj/.ai/src/AGENTS.md", "/proj/CLAUDE.md").unwrap();
        sync_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            "",
            "",
        )
        .unwrap();
        assert!(cleanup_path(&mut s, "/proj/.cursor/rules"));
        assert!(!s.ws.exists("/proj/CLAUDE.md"));
        assert!(s.ws.exists("/proj/.claude/skills/stale"));
        assert!(!s.ws.exists("/proj/.claude/skills/a"));
        assert!(s.ws.exists("/proj/.cursor/rules/core.mdc"));
        assert!(s.touched().is_empty());
        assert_eq!(
            s.log.tail(4),
            [
                "   📁 .ai/src/AGENTS.md → CLAUDE.md (dry-run)",
                "   📁 Would remove: .claude/skills/stale (extraneous)",
                "   📁 .ai/src/skills/ → .claude/skills/ (1 updates, 1 cleanups) (dry-run)",
                "   📁 Would remove: .cursor/rules (dry-run)"
            ]
        );
    }

    #[test]
    fn sync_dir_keeps_an_entry_the_manifest_never_recorded() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.claude/skills/mine/SKILL.md", "m");
        file(&mut s, "/proj/.claude/skills/old/SKILL.md", "o");
        s.activate_manifest(BTreeSet::from([".claude/skills/old/SKILL.md".to_string()]));
        sync_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            "",
            "",
        )
        .unwrap();
        assert!(s.ws.exists("/proj/.claude/skills/mine"));
        assert!(!s.ws.exists("/proj/.claude/skills/old"));
        assert_eq!(s.preserved(), 1);
        assert_eq!(
            s.log.tail(3),
            [
                "[WARNING] Kept .claude/skills/mine (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   📁 Removed: .claude/skills/old",
                "   📁 .ai/src/skills/ → .claude/skills/ (1 updates, 1 cleanups)"
            ]
        );
    }
}
```

- [x] **Step 3: Dry-run and preserve branches in `src/rules.rs`**

Change the last module doc line from `` //! `lib/helpers/format_conversion.sh`, for a forced render. `` to:

```rust
//! `lib/helpers/format_conversion.sh`.
```

Replace `merge_rules_to_file` with:

```rust
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

    if s.dry_run {
        let extra = if agents_file.is_some() {
            " +agents"
        } else {
            ""
        };
        s.log.step(&format!(
            "{src_disp}/ → {dest_disp} ({} files merged{extra}) (dry-run)",
            files.len()
        ));
        return Ok(());
    }
    s.ws.create_dir_all(&paths::parent(dest_file))?;
    let _ = s.ws.remove(dest_file);
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
```

Replace `sync_rules` with:

```rust
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
    if !s.dry_run {
        s.ws.create_dir_all(dest_dir)?;
    }

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
        valid.push(dest_name.clone());
        if s.dry_run {
            continue;
        }
        let dest_path = format!("{dest_dir}/{dest_name}");
        let bytes = read(s, &format!("{src_dir}/{name}"))?;
        let rendered = apply_rule_header(&bytes, opts.header, opts.scoped_header);
        s.ws.write(&dest_path, rendered)?;
        s.record_write(&dest_path);
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
        if !s.may_prune(&path) {
            s.note_preserved(&format!("{dest_disp}/{name}"));
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove: {dest_disp}/{name} (obsolete)"));
        } else {
            s.ws.remove(&path)?;
            s.log.step(&format!("Removed: {dest_disp}/{name}"));
        }
        cleaned += 1;
    }

    let extra = if opts.include.is_empty() {
        String::new()
    } else {
        format!(", include='{}'", opts.include)
    };
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ ({} updates, {cleaned} cleanups){extra}{suffix}",
        valid.len()
    ));
    Ok(())
}
```

Replace `sync_commands_as_skills` with:

```rust
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
    if !s.dry_run {
        s.ws.create_dir_all(dest_dir)?;
    }

    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !filters::matches(&name, include, exclude) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let skill_dir = format!("{dest_dir}/command-{stem}");
        valid.push(format!("command-{stem}"));
        if s.dry_run {
            continue;
        }
        let source = read(s, &format!("{src_dir}/{name}"))?;

        s.ws.create_dir_all(&skill_dir)?;
        let skill_file = format!("{skill_dir}/SKILL.md");
        s.ws.write(&skill_file, convert::command_to_skill(&stem, &source))?;
        s.record_write(&skill_file);

        let policy = format!("{skill_dir}/agents/openai.yaml");
        if convert::read_field(&source, "disable-model-invocation") == b"true" {
            s.ws.create_dir_all(&format!("{skill_dir}/agents"))?;
            s.ws.write(
                &policy,
                b"policy:\n  allow_implicit_invocation: false\n".to_vec(),
            )?;
            s.record_write(&policy);
        } else if s.ws.is_file(&policy) {
            s.ws.remove(&policy)?;
            let agents_dir = format!("{skill_dir}/agents");
            if s.ws.list(&agents_dir).is_empty() {
                let _ = s.ws.remove(&agents_dir);
            }
        }
    }

    for name in s.ws.glob(dest_dir) {
        if !name.starts_with("command-") || !s.ws.is_dir(&format!("{dest_dir}/{name}")) {
            continue;
        }
        if valid.contains(&name) {
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove obsolete generated skill: {name}"));
        } else {
            s.ws.remove(&format!("{dest_dir}/{name}"))?;
            s.log
                .step(&format!("Removed obsolete generated skill: {name}"));
        }
    }
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    s.log.step(&format!(
        "{src_disp}/*.md → {dest_disp}/command-*/SKILL.md ({} generated){suffix}",
        valid.len()
    ));
    Ok(())
}
```

In `impl Conversion`, add before `fn label`:

```rust
    fn dry_run_label(self) -> &'static str {
        match self {
            Self::CommandToml => "md→toml",
            Self::AgentToml => "agent md→toml",
            Self::AgentAmazonqJson => "agent md→json",
            Self::AgentOpencodeMd => "agent md→opencode md",
        }
    }
```

Replace `sync_converted` with:

```rust
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
        valid.push(dest_name);
        let src_file = format!("{src_dir}/{name}");
        if s.dry_run {
            let (src_disp, dest_disp) = (s.display(&src_file), s.display(&dest_file));
            s.log.step(&format!(
                "{src_disp} → {dest_disp} ({}) (dry-run)",
                conversion.dry_run_label()
            ));
            continue;
        }
        let source = read(s, &src_file)?;
        s.ws.create_dir_all(dest_dir)?;
        s.ws.write(&dest_file, conversion.render(&stem, &source))?;
        s.record_write(&dest_file);
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
            if !s.may_prune(&path) {
                s.note_preserved(&format!("{dest_disp}/{name}"));
                continue;
            }
            if s.dry_run {
                s.log
                    .step(&format!("Would remove: {dest_disp}/{name} (obsolete)"));
            } else {
                s.ws.remove(&path)?;
                s.log.step(&format!("Removed: {dest_disp}/{name}"));
            }
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
```

Add this test in the `tests` module, before `fn converted_directories_sweep_their_own_extension_only`:

```rust
    #[test]
    fn dry_runs_and_kept_files_log_what_the_bash_helpers_log() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(
            &mut s,
            "/proj/.ai/src/agents/rev.md",
            "---\nname: rev\n---\nBody\n",
        );
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: R\n---\nBody\n",
        );
        file(&mut s, "/proj/.cursor/rules/old.mdc", "old");
        file(&mut s, "/proj/.cursor/rules/mine.mdc", "mine");
        file(&mut s, "/proj/.codex/agents/old.toml", "old");
        file(&mut s, "/proj/.codex/agents/mine.toml", "mine");
        file(&mut s, "/proj/.agents/skills/command-gone/SKILL.md", "x");
        s.activate_manifest(
            [".cursor/rules/old.mdc", ".codex/agents/old.toml"]
                .map(String::from)
                .into(),
        );
        let opts = RuleOptions {
            extension: ".mdc",
            header: "",
            scoped_header: "",
            include: "",
            exclude: "",
        };

        s.dry_run = true;
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
        )
        .unwrap();
        sync_commands_as_skills(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.agents/skills",
            "",
            "",
        )
        .unwrap();
        merge_rules_to_file(
            &mut s,
            "/proj/.ai/src/rules",
            "/proj/.rules",
            "",
            "",
            Some("/proj/.ai/src/rules/core.md"),
        )
        .unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.gemini/commands",
            Conversion::CommandToml,
        )
        .unwrap();
        assert!(s.ws.exists("/proj/.cursor/rules/old.mdc"));
        assert!(!s.ws.exists("/proj/.rules"));

        s.dry_run = false;
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
        )
        .unwrap();
        assert_eq!(s.ws.list("/proj/.cursor/rules"), ["core.mdc", "mine.mdc"]);
        assert_eq!(s.ws.list("/proj/.codex/agents"), ["mine.toml", "rev.toml"]);
        assert_eq!(s.preserved(), 4);

        let lines: Vec<&str> = s.log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[WARNING] Would keep .cursor/rules/mine.mdc (not from .ai/src/; --force to prune)",
                "   📁 Would remove: .cursor/rules/old.mdc (obsolete)",
                "   📁 .ai/src/rules/ → .cursor/rules/ (1 updates, 1 cleanups) (dry-run)",
                "   📁 .ai/src/agents/rev.md → .codex/agents/rev.toml (agent md→toml) (dry-run)",
                "[WARNING] Would keep .codex/agents/mine.toml (not from .ai/src/; --force to prune)",
                "   📁 Would remove: .codex/agents/old.toml (obsolete)",
                "   📁 .ai/src/agents/ → .codex/agents/ (1 agents, md→toml)",
                "   📁 Would remove obsolete generated skill: command-gone",
                "   📁 .ai/src/commands/*.md → .agents/skills/command-*/SKILL.md (1 generated) (dry-run)",
                "   📁 .ai/src/rules/ → .rules (1 files merged +agents) (dry-run)",
                "   📁 .ai/src/commands/review.md → .gemini/commands/review.toml (md→toml) (dry-run)",
                "   📁 .ai/src/commands/ → .gemini/commands/ (1 commands, md→toml)",
                "[WARNING] Kept .cursor/rules/mine.mdc (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   📁 Removed: .cursor/rules/old.mdc",
                "   📁 .ai/src/rules/ → .cursor/rules/ (1 updates, 1 cleanups)",
                "[WARNING] Kept .codex/agents/mine.toml (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   📁 Removed: .codex/agents/old.toml",
                "   📁 .ai/src/agents/ → .codex/agents/ (1 agents, md→toml)",
            ]
        );
    }
```

- [x] **Step 4: The render's own dry-run branches in `src/render.rs`**

In `sync_rules_step`, replace:

```rust
    if tool.value("targets.rules.inline_into_agents") == "true" && !dests.agents.is_empty() {
        inline_rules_into_agents(s, &src_rules, &dests.agents, &include, &exclude)?;
    } else if !dests.rules.is_empty() {
```

with:

```rust
    if tool.value("targets.rules.inline_into_agents") == "true" && !dests.agents.is_empty() {
        if s.dry_run {
            s.log.step(&format!(
                "Would append rule references to {} (dry-run)",
                paths::leaf(&dests.agents)
            ));
        } else {
            inline_rules_into_agents(s, &src_rules, &dests.agents, &include, &exclude)?;
        }
    } else if !dests.rules.is_empty() {
```

then `if tool.value("targets.rules.append_imports") == "true" {` with:

```rust
            if tool.value("targets.rules.append_imports") == "true" && !s.dry_run {
```

and the nested-agents condition with:

```rust
    if !dests.agents.is_empty()
        && !dests.rules.is_empty()
        && dests.agents.starts_with(&format!("{}/", dests.rules))
        && !s.dry_run
    {
```

In `sync_skills_step`, replace:

```rust
        if !target.is_empty() {
            inline_skills_into_file(s, &src_skills, &target, &include, &exclude)?;
        }
```

with:

```rust
        if !target.is_empty() && !s.dry_run {
            inline_skills_into_file(s, &src_skills, &target, &include, &exclude)?;
        } else if s.dry_run {
            s.log.step("Would append skill index (dry-run)");
        }
```

In `sync_commands_step`, replace `        if !target.is_empty() {` (the one before the `has no native commands surface — appending command index` line) with:

```rust
        if !target.is_empty() && s.dry_run {
            s.log.step("Would append command index (dry-run)");
        } else if !target.is_empty() {
```

In `sync_payloads_step`, replace:

```rust
        if let Some(src) = payload::resolve_source(s, tool, resource).filter(|p| s.ws.is_file(p)) {
            file_ops::copy_file(s, &src, dest).map_err(|e| io(s, e))?;
        }
```

with:

```rust
        if let Some(src) = payload::resolve_source(s, tool, resource).filter(|p| s.ws.is_file(p)) {
            file_ops::copy_file(s, &src, dest).map_err(|e| io(s, e))?;
            if resource == "guard" && !s.dry_run {
                s.ws.make_executable(dest).map_err(|e| io(s, e))?;
            }
        }
```

In `compose_opencode`, add this arm before `Ok(composed) => {`:

```rust
        Ok(_) if s.dry_run => {
            s.log.step(&format!(
                "Would compose OpenCode settings and MCP → {} (dry-run)",
                s.display(dest)
            ));
            Ok(())
        }
```

- [x] **Step 5: `make_executable` in `src/workspace.rs`**

Add before the doc comment of `copy` (`` /// `cp -r src dst` onto a missing `dst` ``):

```rust
    /// `chmod +x` as the default umask applies it; the in-memory tree has no modes.
    pub fn make_executable(&mut self, path: &str) -> Result<(), Error> {
        #[cfg(unix)]
        if self.writes_disk(path) {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)
                .map_err(|e| Error::io(path, e))?
                .permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            return std::fs::set_permissions(path, permissions).map_err(|e| Error::io(path, e));
        }
        Ok(())
    }
```

- [x] **Step 6: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: `120 passed` (unit) and `7 passed` (integration); fmt and clippy exit 0.

- [x] **Step 7: Cross-check the asserted lines against the Bash helpers**

```bash
bash -c '
set -euo pipefail
ENGINE=$PWD; dir=$(mktemp -d "${TMPDIR:-/tmp}/cross.XXXXXX"); REPO_ROOT="$dir"; REPO_ROOT_CANONICAL=$(cd -P "$dir" && pwd)
DEFAULT_REPO_ROOT=$ENGINE; HOME=/nonexistent
for h in logging tmp yaml paths filters file_ops rule_operations format_conversion manifest; do source "$ENGINE/lib/helpers/$h.sh"; done
tmp_prime_run_dir; cd "$dir"
mkdir -p .ai/src/rules .ai/src/agents .ai/src/commands .cursor/rules .codex/agents .agents/skills/command-gone
echo "# Core" > .ai/src/rules/core.md
printf -- "---\nname: rev\n---\nBody\n" > .ai/src/agents/rev.md
printf -- "---\ndescription: R\n---\nBody\n" > .ai/src/commands/review.md
echo old > .cursor/rules/old.mdc; echo mine > .cursor/rules/mine.mdc
echo old > .codex/agents/old.toml; echo mine > .codex/agents/mine.toml
printf ".cursor/rules/old.mdc\th\n.codex/agents/old.toml\th\n" > .ai/.sync-manifest
manifest_load; SYNC_MANIFEST_ACTIVE=true; FORCE_SYNC=false
sync_rules "$dir/.ai/src/rules" "$dir/.cursor/rules" ".mdc" "" "true" "" ""
sync_agents_as_toml "$dir/.ai/src/agents" "$dir/.codex/agents" "true"
sync_commands_as_skills "$dir/.ai/src/commands" "$dir/.agents/skills" "true" "" ""
merge_rules_to_file "$dir/.ai/src/rules" "$dir/.rules" "true" "" "" "$dir/.ai/src/rules/core.md"
sync_commands_as_toml "$dir/.ai/src/commands" "$dir/.gemini/commands" "true"
sync_rules "$dir/.ai/src/rules" "$dir/.cursor/rules" ".mdc" "" "false" "" ""
sync_agents_as_toml "$dir/.ai/src/agents" "$dir/.codex/agents" "false"
tmp_cleanup; rm -rf "$dir"'
```

Expected: the eighteen lines of `dry_runs_and_kept_files_log_what_the_bash_helpers_log`, in its order.

- [x] **Step 8: Commit**

```bash
git add src/session.rs src/file_ops.rs src/rules.rs src/render.rs src/workspace.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): honour --dry-run and keep untracked outputs in every render step"
```

---

### Task 3: Sync Manifest

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock` (`sha2`, decision 2)
- Create: `src/staging.rs`
- Create: `src/manifest.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Log` (Task 1).
- Produces: `staging::write_beside(dest: &Path, bytes: &[u8]) -> Result<(), Error>` — `tmp_sibling` + `mv`: the staging file is created `0600` and takes an existing destination's mode. `manifest::REL` (`".ai/.sync-manifest"`); `Manifest::load(root: &str) -> Result<Option<Manifest>, Error>` (`None` makes the run a baseline); `Manifest::parse(bytes: &[u8]) -> Manifest` (`IFS=$'\t' read -r rel hash`); `Manifest::paths(&self) -> BTreeSet<String>`; `Manifest::drift(&self, root: &str) -> Vec<String>` (manifest order; a missing file is not drift); `manifest::sha256_hex(bytes: &[u8]) -> String`; `manifest::write(root: &str, previous: Option<&Manifest>, touched: &BTreeSet<String>, log: &mut Log) -> Result<(), Error>` with the `Removed …` and `Initialized …` lines.

- [x] **Step 1: Add the digest crate**

Run: `cargo add sha2@0.11 --no-default-features`
Expected: `Cargo.toml` gains `sha2 = { version = "0.11", default-features = false }` between `include_dir` and `thiserror`.

- [x] **Step 2: Create `src/staging.rs`**

```rust
//! `tmp_sibling` + `mv` from `lib/helpers/tmp.sh`: a file is replaced by
//! renaming a staging file written beside it, so a reader never sees it half
//! written and the rename stays on one filesystem.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Error;

/// Replaces `dest` with `bytes`. The staging file is created `0600`, as
/// `mktemp` creates it, and takes the mode of an existing `dest`.
pub fn write_beside(dest: &Path, bytes: &[u8]) -> Result<(), Error> {
    let (path, mut file) = create_sibling(dest)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| match std::fs::metadata(dest) {
            Ok(meta) if meta.is_file() => std::fs::set_permissions(&path, meta.permissions()),
            _ => Ok(()),
        })
        .and_then(|()| std::fs::rename(&path, dest));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(Error::io(dest, e));
    }
    Ok(())
}

fn create_sibling(dest: &Path) -> Result<(PathBuf, File), Error> {
    let pid = std::process::id();
    let mut attempt = 0u64;
    loop {
        let mut name = dest.as_os_str().to_owned();
        name.push(format!(".{pid}{attempt:04}"));
        let path = PathBuf::from(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(Error::io(dest, e)),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_file_is_private_like_mktemp_and_a_replaced_file_keeps_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join(".sync-manifest");
        write_beside(&fresh, b"a\th\n").unwrap();
        assert_eq!(std::fs::read(&fresh).unwrap(), b"a\th\n");
        assert_eq!(mode(&fresh), 0o600);

        let kept = dir.path().join(".gitignore");
        std::fs::write(&kept, "old\n").unwrap();
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_beside(&kept, b"new\n").unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"new\n");
        assert_eq!(mode(&kept), 0o644);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_missing_parent_is_an_error_that_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_beside(&dir.path().join("missing/x"), b"x").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
```

- [x] **Step 3: Create `src/manifest.rs`**

```rust
//! `.ai/.sync-manifest` as `lib/helpers/manifest.sh` reads and writes it: one
//! `<rel>\t<sha256>` line per output, `LC_ALL=C sort -u`, no header.

use std::collections::BTreeSet;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::log::Log;
use crate::{Error, staging};

pub const REL: &str = ".ai/.sync-manifest";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    entries: Vec<(String, String)>,
}

impl Manifest {
    /// `manifest_load`: `None` when no manifest exists, which makes the run a
    /// baseline initialisation.
    pub fn load(root: &str) -> Result<Option<Self>, Error> {
        let path = Path::new(root).join(REL);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        Ok(Some(Self::parse(&bytes)))
    }

    /// `IFS=$'\t' read -r rel hash` per line: tabs around the line are dropped,
    /// the hash is the rest after the first run of tabs, and comments and
    /// entries without a hash are skipped.
    pub fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let mut entries = Vec::new();
        for line in text.split('\n') {
            let line = line.trim_matches('\t');
            let (rel, hash) = match line.find('\t') {
                Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
                None => (line, ""),
            };
            if rel.is_empty() || rel.starts_with('#') || hash.is_empty() {
                continue;
            }
            entries.push((rel.to_string(), hash.to_string()));
        }
        Self { entries }
    }

    pub fn paths(&self) -> BTreeSet<String> {
        self.entries.iter().map(|(rel, _)| rel.clone()).collect()
    }

    /// `manifest_check_drift`: entries whose file exists with another hash, in
    /// manifest order. A missing file is not drift.
    pub fn drift(&self, root: &str) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(rel, old)| {
                hash_file(&Path::new(root).join(rel)).is_some_and(|current| current != *old)
            })
            .map(|(rel, _)| rel.clone())
            .collect()
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_file(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// `manifest_write`: previous entries whose file still exists and this run did
/// not touch, plus fresh hashes of every touched file that exists. An empty
/// result removes the manifest.
pub fn write(
    root: &str,
    previous: Option<&Manifest>,
    touched: &BTreeSet<String>,
    log: &mut Log,
) -> Result<(), Error> {
    let exists = |rel: &str| Path::new(root).join(rel).is_file();
    let mut lines: BTreeSet<String> = BTreeSet::new();
    for (rel, hash) in previous.map(|m| m.entries.as_slice()).unwrap_or_default() {
        if exists(rel) && !touched.contains(rel) {
            lines.insert(format!("{rel}\t{hash}"));
        }
    }
    for rel in touched {
        if let Some(hash) = hash_file(&Path::new(root).join(rel)) {
            lines.insert(format!("{rel}\t{hash}"));
        }
    }

    let path = Path::new(root).join(REL);
    if lines.is_empty() {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
            log.info("Removed .ai/.sync-manifest (no tracked outputs)");
        }
        return Ok(());
    }
    let ai = Path::new(root).join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    let mut text = lines
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    staging::write_beside(&path, text.as_bytes())?;
    if previous.is_none() {
        log.info(&format!(
            "Initialized .ai/.sync-manifest with {} entries — commit it to track drift in CI",
            lines.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_match_sha256sum() {
        assert_eq!(
            sha256_hex(b"hello\n"),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn lines_are_read_the_way_bash_read_splits_them_on_tabs() {
        let manifest =
            Manifest::parse(b"a.md\th1\n\tb.md\th2\t\n#c\th\nd.md\n\ne.md\th\te\nlast\th9");
        assert_eq!(
            manifest.entries,
            [
                ("a.md".to_string(), "h1".to_string()),
                ("b.md".to_string(), "h2".to_string()),
                ("e.md".to_string(), "h\te".to_string()),
                ("last".to_string(), "h9".to_string()),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn drift_is_a_changed_file_in_manifest_order_and_a_missing_file_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("b.md"), "edited\n").unwrap();
        std::fs::write(dir.path().join("a.md"), "hello\n").unwrap();
        std::fs::write(dir.path().join("c.md"), "edited\n").unwrap();
        let text = format!(
            "c.md\t{0}\nb.md\t{0}\na.md\t{0}\ngone.md\tx\n",
            sha256_hex(b"hello\n")
        );
        assert_eq!(
            Manifest::parse(text.as_bytes()).drift(&root),
            ["c.md", "b.md"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn writing_keeps_untouched_entries_hashes_touched_files_and_sorts_bytewise() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "hello\n").unwrap();
        std::fs::write(dir.path().join(".claude/x.md"), "").unwrap();
        std::fs::write(dir.path().join("skipped.md"), "stale\n").unwrap();
        let previous = Manifest::parse(b"skipped.md\told\ngone.md\told\nCLAUDE.md\told\n");
        let touched = BTreeSet::from(["CLAUDE.md".to_string(), ".claude/x.md".to_string()]);

        let mut log = Log::default();
        write(&root, Some(&previous), &touched, &mut log).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            format!(
                ".claude/x.md\t{}\nCLAUDE.md\t{}\nskipped.md\told\n",
                sha256_hex(b""),
                sha256_hex(b"hello\n")
            )
        );
        assert!(log.lines().is_empty());

        write(&root, None, &touched, &mut log).unwrap();
        assert_eq!(
            log.tail(1),
            [
                "[INFO] Initialized .ai/.sync-manifest with 2 entries — commit it to track drift in CI"
            ]
        );

        std::fs::remove_file(dir.path().join("CLAUDE.md")).unwrap();
        std::fs::remove_file(dir.path().join(".claude/x.md")).unwrap();
        write(&root, Some(&previous), &BTreeSet::new(), &mut log).unwrap();
        std::fs::remove_file(dir.path().join("skipped.md")).unwrap();
        write(&root, Some(&previous), &BTreeSet::new(), &mut log).unwrap();
        assert!(!dir.path().join(REL).exists());
        assert_eq!(
            log.tail(1),
            ["[INFO] Removed .ai/.sync-manifest (no tracked outputs)"]
        );
    }
}
```

- [x] **Step 4: Declare the modules in `src/lib.rs`**

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
pub mod manifest;
pub mod opencode_json;
pub mod overlay;
pub mod paths;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod render;
pub mod rules;
pub mod session;
pub mod staging;
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

- [x] **Step 5: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: `126 passed` (unit) and `7 passed` (integration); fmt and clippy exit 0.

- [x] **Step 6: Cross-check the digests and the line reader against Bash**

```bash
printf 'hello\n' | shasum -a 256
printf '' | shasum -a 256
bash -c 'm=$(mktemp "${TMPDIR:-/tmp}/manifest.XXXXXX")
printf "a.md\th1\n\tb.md\th2\t\n#c\th\nd.md\n\ne.md\th\te\nlast\th9" > "$m"
MANIFEST_KEYS=(); MANIFEST_VALUES=(); source lib/helpers/manifest.sh; manifest_path() { echo "$m"; }
manifest_load; for i in "${!MANIFEST_KEYS[@]}"; do printf "[%s]=[%s]\n" "${MANIFEST_KEYS[$i]}" "${MANIFEST_VALUES[$i]}"; done; rm -f "$m"' | sed -n l
```

Expected: `5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  -` and `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  -`; then `[a.md]=[h1]$`, `[b.md]=[h2]$`, `[e.md]=[h\te]$`, `[last]=[h9]$` — the entries `lines_are_read_the_way_bash_read_splits_them_on_tabs` asserts.

- [x] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/staging.rs src/manifest.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): load, check, and write the sync manifest"
```

---

### Task 4: Managed `.gitignore` Block

**Files:**
- Create: `src/gitignore.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `staging::write_beside` (Task 3); Task 0d's byte order.
- Produces: `gitignore::has_managed_block(path: &Path) -> bool`; `gitignore::update(path: &Path, paths: &[String], log: &mut Log) -> Result<(), Error>` — replaces the block when both markers are present, otherwise appends a fresh one after a blank line, warning when only one marker is.

- [ ] **Step 1: Create `src/gitignore.rs`**

```rust
//! The managed `.gitignore` block of `lib/helpers/gitignore.sh`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::log::Log;
use crate::{Error, staging, text};

const START: &str = "# --- AI SYNC GENERATED START ---";
const END: &str = "# --- AI SYNC GENERATED END ---";

/// `gitignore_has_managed_block`.
pub fn has_managed_block(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| contains(&bytes, START))
}

fn contains(bytes: &[u8], marker: &str) -> bool {
    bytes
        .windows(marker.len())
        .any(|window| window == marker.as_bytes())
}

/// `update_gitignore`: replace the block between the markers, or append a
/// fresh one when either marker is missing.
pub fn update(path: &Path, paths: &[String], log: &mut Log) -> Result<(), Error> {
    let io = |e| Error::io(path, e);
    if !path.is_file() {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
    }
    let mut block = format!(
        "{START}\n# Automatically generated by lib/sync.sh\n# Do not edit this block manually.\n"
    );
    let sorted: BTreeSet<&str> = paths
        .iter()
        .map(String::as_str)
        .filter(|p| !p.is_empty())
        .collect();
    for entry in sorted {
        block.push_str(entry);
        block.push('\n');
    }
    block.push_str(END);

    let current = std::fs::read(path).map_err(io)?;
    let (has_start, has_end) = (contains(&current, START), contains(&current, END));
    if has_start && has_end {
        let mut out = Vec::with_capacity(current.len() + block.len());
        let mut skip = false;
        for line in text::lines(&current) {
            if line == START.as_bytes() {
                out.extend_from_slice(block.as_bytes());
                out.push(b'\n');
                skip = true;
            } else if line == END.as_bytes() {
                skip = false;
            } else if !skip {
                out.extend_from_slice(line);
                out.push(b'\n');
            }
        }
        staging::write_beside(path, &out)?;
        log.step("Updated .gitignore block");
        return Ok(());
    }
    if has_start || has_end {
        log.warning("Detected inconsistent .gitignore markers. Appending a fresh generated block.");
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(format!("\n{block}\n").as_bytes()))
        .map_err(io)?;
    log.step("Added generated block to .gitignore");
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn a_missing_file_gets_a_block_after_a_blank_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        let mut log = Log::default();
        update(
            &path,
            &paths(&[".cursor/", ".claude/", ".cursor/", "", "B/", "_x/", "b/"]),
            &mut log,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\n# --- AI SYNC GENERATED START ---\n# Automatically generated by lib/sync.sh\n# Do not edit this block manually.\n.claude/\n.cursor/\nB/\n_x/\nb/\n# --- AI SYNC GENERATED END ---\n"
        );
        assert_eq!(log.tail(1), ["   📁 Added generated block to .gitignore"]);
        assert!(has_managed_block(&path));
    }

    #[test]
    fn a_rerun_replaces_only_the_block_and_keeps_user_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "node_modules/\n").unwrap();
        let mut log = Log::default();
        update(&path, &paths(&[".claude/"]), &mut log).unwrap();
        std::fs::write(
            &path,
            format!("{}dist/", std::fs::read_to_string(&path).unwrap()),
        )
        .unwrap();
        update(&path, &[], &mut log).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "node_modules/\n\n# --- AI SYNC GENERATED START ---\n# Automatically generated by lib/sync.sh\n# Do not edit this block manually.\n# --- AI SYNC GENERATED END ---\ndist/\n"
        );
        assert_eq!(log.tail(1), ["   📁 Updated .gitignore block"]);
    }

    #[test]
    fn a_lone_marker_appends_a_fresh_block_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "# --- AI SYNC GENERATED START ---\nx\n").unwrap();
        let mut log = Log::default();
        update(&path, &paths(&["y/"]), &mut log).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with("x\n\n# --- AI SYNC GENERATED START ---\n# Automatically generated by lib/sync.sh\n# Do not edit this block manually.\ny/\n# --- AI SYNC GENERATED END ---\n")
        );
        assert_eq!(
            log.tail(2),
            [
                "[WARNING] Detected inconsistent .gitignore markers. Appending a fresh generated block.",
                "   📁 Added generated block to .gitignore"
            ]
        );
    }
}
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod file_ops;
pub mod filters;
pub mod gitignore;
pub mod log;
pub mod manifest;
pub mod opencode_json;
pub mod overlay;
pub mod paths;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod render;
pub mod rules;
pub mod session;
pub mod staging;
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

- [ ] **Step 3: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: `129 passed` (unit) and `7 passed` (integration); fmt and clippy exit 0.

- [ ] **Step 4: Cross-check the three cases against `update_gitignore`**

```bash
bash -c 'set -euo pipefail
source lib/helpers/logging.sh; source lib/helpers/tmp.sh; source lib/helpers/gitignore.sh
d=$(mktemp -d "${TMPDIR:-/tmp}/gi.XXXXXX"); tmp_prime_run_dir
update_gitignore "$d/a" "$(printf "%s\n" .cursor/ .claude/ .cursor/ "" B/ _x/ b/)"; cat -e "$d/a"; echo ===
printf "node_modules/\n" > "$d/b"; update_gitignore "$d/b" ".claude/"; printf "dist/" >> "$d/b"; update_gitignore "$d/b" ""; cat -e "$d/b"; echo ===
printf "# --- AI SYNC GENERATED START ---\nx\n" > "$d/c"; update_gitignore "$d/c" "y/"; cat -e "$d/c"
tmp_cleanup; rm -rf "$d"'
```

Expected: the file contents and log lines the three `gitignore.rs` tests assert — the first file opens with an empty line and lists `.claude/`, `.cursor/`, `B/`, `_x/`, `b/`; the second keeps `node_modules/` above and `dist/` below an emptied block and logs `Updated .gitignore block`; the third logs the inconsistent-markers warning before `Added generated block to .gitignore`.

- [ ] **Step 5: Commit**

```bash
git add src/gitignore.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): update the managed .gitignore block"
```

---

### Task 5: Backups

**Files:**
- Create: `src/backup.rs`
- Modify: `src/error.rs` (`Error::Backup`)
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `paths::{parent, leaf, is_within}`.
- Produces: `Error::Backup(String)` — a refusal of `backup.sh`, whose `Display` is the message a caller prints after `Error: `. `backup::Target { present: bool, rel: String }`. `backup::canonical_root(root: &str) -> Result<String, Error>`; `backup::create(root: &str, operation: &str, targets: &[String]) -> Result<String, Error>` (the canonical snapshot path); `backup::snapshot_path(root: &str, requested: &str) -> Result<String, Error>`; `backup::load_targets(root: &str, requested: &str) -> Result<Vec<Target>, Error>`; `backup::restore(root: &str, requested: &str) -> Result<(), Error>`; `backup::latest(root: &str) -> Result<Option<String>, Error>`; `backup::list(root: &str) -> Result<Vec<(String, String, String)>, Error>` (id, operation, created); `backup::prune(root: &str, limit: Option<&str>, max_age: Option<&str>) -> Result<(), Error>` (the raw `AGENTSYNC_BACKUP_LIMIT` and `AGENTSYNC_BACKUP_MAX_AGE_DAYS`). Snapshot directories are `0700`, `.latest` and the store's `.gitignore` `0600`, as `mktemp` creates them; links stay links and modes and modification times are kept, as `tar` and `cp -pPR` keep them.

- [ ] **Step 1: Create `src/backup.rs`**

```rust
//! Transactional backups of `lib/helpers/backup.sh`, in its on-disk layout:
//! `.ai/backups/<UTC stamp>-<operation>-<pid>[-n]/` holding `metadata`,
//! `targets.tsv`, a `files/` mirror, and `.complete`; the store keeps `.latest`
//! and a `.gitignore` of `*`. A Bash `rollback` reads what this writes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Error, paths};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub present: bool,
    pub rel: String,
}

fn refuse(message: impl Into<String>) -> Error {
    Error::Backup(message.into())
}

/// `_backup_canonical_root`.
pub fn canonical_root(root: &str) -> Result<String, Error> {
    if !Path::new(root).is_dir() {
        return Err(refuse(format!("Backup root is not a directory: {root}")));
    }
    canonical_dir(Path::new(root)).map_err(|e| Error::io(root, e))
}

fn canonical_dir(dir: &Path) -> std::io::Result<String> {
    std::fs::canonicalize(dir).map(|p| p.to_string_lossy().into_owned())
}

fn exists_or_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `_backup_validate_rel`.
fn validate_rel(rel: &str, allow_store_parent: bool) -> Result<(), Error> {
    if rel.is_empty() || rel == "." || rel.starts_with('/') {
        let shown = if rel.is_empty() { "<empty>" } else { rel };
        return Err(refuse(format!("Refusing unsafe backup target: {shown}")));
    }
    let wrapped = format!("/{rel}/");
    if wrapped.contains("/../") || wrapped.contains("/./") {
        return Err(refuse(format!(
            "Refusing non-normalized backup target: {rel}"
        )));
    }
    if rel == ".ai" && !allow_store_parent {
        return Err(refuse(format!(
            "Refusing to back up a parent of the backup store: {rel}"
        )));
    }
    if rel == ".ai/backups" || rel.starts_with(".ai/backups/") {
        return Err(refuse(format!(
            "Refusing to back up the backup store itself: {rel}"
        )));
    }
    if rel.contains(['\t', '\n', '\r']) {
        return Err(refuse("Backup targets cannot contain tabs or newlines"));
    }
    Ok(())
}

/// `_backup_safe_target_path_r`: `<canonical_root>/<rel>`, once its nearest
/// existing ancestor is a directory inside the root.
fn safe_target_path(
    canonical_root: &str,
    rel: &str,
    allow_store_parent: bool,
) -> Result<String, Error> {
    validate_rel(rel, allow_store_parent)?;
    let abs = format!("{canonical_root}/{rel}");
    let mut probe = paths::parent(&abs);
    while !exists_or_link(Path::new(&probe)) {
        let up = paths::parent(&probe);
        if up == probe {
            break;
        }
        probe = up;
    }
    if !Path::new(&probe).is_dir() {
        return Err(refuse(format!(
            "Backup target parent is not a directory: {rel}"
        )));
    }
    let resolved = canonical_dir(Path::new(&probe))
        .map_err(|_| refuse(format!("Could not resolve backup target parent: {rel}")))?;
    if !paths::is_within(&resolved, canonical_root) {
        return Err(refuse(format!(
            "Backup target resolves outside the repository root: {rel}"
        )));
    }
    Ok(abs)
}

/// `_backup_target_abs_r`.
fn target_abs(supplied_root: &str, canonical_root: &str, target: &str) -> Result<String, Error> {
    if target == supplied_root || target == canonical_root {
        return Err(refuse("Refusing to back up the repository root"));
    }
    let rel = if let Some(rest) = target.strip_prefix(&format!("{supplied_root}/")) {
        rest
    } else if let Some(rest) = target.strip_prefix(&format!("{canonical_root}/")) {
        rest
    } else if !target.starts_with('/') {
        target
    } else {
        return Err(refuse(format!(
            "Backup target is outside the repository root: {target}"
        )));
    };
    validate_rel(rel, false)?;
    safe_target_path(canonical_root, rel, false)
}

/// `_backup_prepare_targets`: validated, deduplicated, and collapsed into their
/// shallowest roots, in first-seen order.
fn prepare_targets(supplied_root: &str, targets: &[String]) -> Result<Vec<String>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let mut prepared: Vec<String> = Vec::new();
    for target in targets {
        let candidate = target_abs(supplied_root, &canonical, target)?;
        let covered = prepared
            .iter()
            .any(|existing| paths::is_within(&candidate, existing));
        prepared.retain(|existing| !existing.starts_with(&format!("{candidate}/")));
        if !covered {
            prepared.push(candidate);
        }
    }
    if prepared.is_empty() {
        return Err(refuse("No backup targets were provided"));
    }
    Ok(prepared)
}

/// `_backup_validate_store`.
fn validate_store(canonical_root: &str) -> Result<String, Error> {
    safe_target_path(canonical_root, ".ai", true)?;
    let ai = PathBuf::from(format!("{canonical_root}/.ai"));
    let store = PathBuf::from(format!("{canonical_root}/.ai/backups"));
    if ai.is_symlink() {
        return Err(refuse("AgentSync state directory cannot be a symlink: .ai"));
    }
    if ai.exists() && !ai.is_dir() {
        return Err(refuse("AgentSync state path is not a directory: .ai"));
    }
    if ai.is_dir() && canonical_dir(&ai).ok().as_deref() != Some(&format!("{canonical_root}/.ai")) {
        return Err(refuse(
            "AgentSync state directory resolves outside the repository root",
        ));
    }
    if store.is_symlink() {
        return Err(refuse("Backup store cannot be a symlink: .ai/backups"));
    }
    if store.exists() && !store.is_dir() {
        return Err(refuse("Backup store is not a directory: .ai/backups"));
    }
    if store.is_dir()
        && canonical_dir(&store).ok().as_deref() != Some(&format!("{canonical_root}/.ai/backups"))
    {
        return Err(refuse("Backup store resolves outside the repository root"));
    }
    Ok(store.to_string_lossy().into_owned())
}

/// `mktemp "$store/<prefix>XXXXXX"` then `mv` onto `<store>/<name>`: a
/// symlink at either path is replaced, never followed.
fn write_store_file(store: &str, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let staging = create_unique(
        store,
        &format!(".{}.tmp.", name.trim_start_matches('.')),
        false,
    )?;
    let written = OpenOptions::new()
        .write(true)
        .open(&staging)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&staging, format!("{store}/{name}")));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(Error::io(&staging, e));
    }
    Ok(())
}

/// `mktemp [-d] "<dir>/<prefix>XXXXXX"`: created exclusively, mode `0600` or `0700`.
fn create_unique(dir: &str, prefix: &str, directory: bool) -> Result<PathBuf, Error> {
    let pid = std::process::id();
    let mut attempt = 0u64;
    loop {
        let path = PathBuf::from(format!("{dir}/{prefix}{pid}{attempt:04}"));
        let created = if directory {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            builder.create(&path)
        } else {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            options.open(&path).map(|_| ())
        };
        match created {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(Error::io(&path, e)),
        }
    }
}

/// `_backup_sweep_stale_staging`: staging older than a day, left by a run that
/// died before its `mv`.
fn sweep_stale_staging(store: &str, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(store) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(".tmp.")
            || name.starts_with(".latest.tmp.")
            || name.starts_with(".gitignore.tmp."))
        {
            continue;
        }
        let stale = std::fs::symlink_metadata(entry.path())
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= Duration::from_secs(24 * 60 * 60));
        if stale {
            let _ = remove_all(&entry.path());
        }
    }
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// `cp -pPR src dst` and the `tar` pipe: links stay links; modes and
/// modification times are kept.
fn copy_preserving(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_symlink() {
        let link = std::fs::read_link(src)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(link, dst);
        #[cfg(windows)]
        return if std::fs::metadata(src).is_ok_and(|m| m.is_dir()) {
            std::os::windows::fs::symlink_dir(link, dst)
        } else {
            std::os::windows::fs::symlink_file(link, dst)
        };
    }
    if meta.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_preserving(&entry.path(), &dst.join(entry.file_name()))?;
        }
        std::fs::set_permissions(dst, meta.permissions())?;
        #[cfg(unix)]
        std::fs::File::open(dst)?.set_modified(meta.modified()?)?;
        return Ok(());
    }
    std::fs::copy(src, dst)?;
    std::fs::File::open(dst)?.set_modified(meta.modified()?)
}

/// `backup_create`: the snapshot's path.
pub fn create(supplied_root: &str, operation: &str, targets: &[String]) -> Result<String, Error> {
    create_at(supplied_root, operation, targets, SystemTime::now())
}

fn create_at(
    supplied_root: &str,
    operation: &str,
    targets: &[String],
    now: SystemTime,
) -> Result<String, Error> {
    let valid_operation = operation
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && operation
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_operation {
        return Err(refuse(format!(
            "Invalid backup operation name: {operation}"
        )));
    }
    let canonical = canonical_root(supplied_root)?;
    let prepared = prepare_targets(supplied_root, targets)?;
    let store = validate_store(&canonical)?;
    std::fs::create_dir_all(&store).map_err(|e| Error::io(&store, e))?;
    let store = validate_store(&canonical)?;
    write_store_file(&store, ".gitignore", b"*\n")?;
    sweep_stale_staging(&store, now);

    let stage = create_unique(
        &store,
        &format!(".tmp.{operation}.{}.", std::process::id()),
        true,
    )?;
    let id = match stage_snapshot(&stage, &canonical, &prepared, operation, now) {
        Ok(id) => id,
        Err(e) => {
            let _ = remove_all(&stage);
            return Err(e);
        }
    };
    let mut snapshot_id = id.clone();
    let mut counter = 1;
    while Path::new(&format!("{store}/{snapshot_id}")).exists() {
        counter += 1;
        snapshot_id = format!("{id}-{counter}");
    }
    let snapshot = format!("{store}/{snapshot_id}");
    if let Err(e) = std::fs::rename(&stage, &snapshot) {
        let _ = remove_all(&stage);
        return Err(Error::io(&snapshot, e));
    }
    if let Err(e) = write_store_file(&store, ".latest", format!("{snapshot_id}\n").as_bytes()) {
        let _ = remove_all(Path::new(&snapshot));
        return Err(e);
    }
    Ok(snapshot)
}

fn stage_snapshot(
    stage: &Path,
    canonical_root: &str,
    prepared: &[String],
    operation: &str,
    now: SystemTime,
) -> Result<String, Error> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |e| Error::io(path, e)
    };
    let files = stage.join("files");
    std::fs::create_dir(&files).map_err(io(&files))?;
    let created = utc_stamp(now);
    let metadata = stage.join("metadata");
    std::fs::write(
        &metadata,
        format!("schema=1\noperation={operation}\ncreated_at={created}\n"),
    )
    .map_err(io(&metadata))?;

    let mut records = String::new();
    for abs in prepared {
        let rel = abs
            .strip_prefix(&format!("{canonical_root}/"))
            .unwrap_or(abs);
        let source = Path::new(abs);
        let present = exists_or_link(source);
        records.push_str(if present { "present\t" } else { "missing\t" });
        records.push_str(rel);
        records.push('\n');
        if present {
            let dest = files.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            copy_preserving(source, &dest).map_err(io(source))?;
        }
    }
    let targets = stage.join("targets.tsv");
    std::fs::write(&targets, records).map_err(io(&targets))?;
    let complete = stage.join(".complete");
    std::fs::write(&complete, "").map_err(io(&complete))?;
    Ok(format!("{created}-{operation}-{}", std::process::id()))
}

/// `_backup_snapshot_path`.
pub fn snapshot_path(supplied_root: &str, requested: &str) -> Result<String, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    let id = paths::leaf(requested);
    if id.is_empty() || id == "." || id == ".." {
        return Err(refuse(format!("Invalid backup snapshot: {requested}")));
    }
    let snapshot = format!("{store}/{id}");
    let path = Path::new(&snapshot);
    if path.is_symlink()
        || !path.is_dir()
        || !path.join(".complete").is_file()
        || !path.join("targets.tsv").is_file()
    {
        return Err(refuse(format!(
            "Backup snapshot is missing or incomplete: {id}"
        )));
    }
    if canonical_dir(path).ok().as_deref() != Some(snapshot.as_str()) {
        return Err(refuse(format!(
            "Backup snapshot resolves outside the backup store: {id}"
        )));
    }
    Ok(snapshot)
}

/// `_backup_snapshot_source`.
fn snapshot_source(snapshot: &str, rel: &str) -> Result<String, Error> {
    let files_root = format!("{snapshot}/files");
    let files = Path::new(&files_root);
    if files.is_symlink()
        || !files.is_dir()
        || canonical_dir(files).ok().as_deref() != Some(files_root.as_str())
    {
        return Err(refuse("Snapshot files directory is unsafe"));
    }
    let source = format!("{files_root}/{rel}");
    let mut probe = paths::parent(&source);
    while !exists_or_link(Path::new(&probe)) {
        let up = paths::parent(&probe);
        if up == probe {
            break;
        }
        probe = up;
    }
    if !Path::new(&probe).is_dir() {
        return Err(refuse(format!(
            "Snapshot source parent is not a directory: {rel}"
        )));
    }
    let resolved = canonical_dir(Path::new(&probe)).map_err(|e| Error::io(&probe, e))?;
    if !paths::is_within(&resolved, &files_root) {
        return Err(refuse(format!(
            "Snapshot source resolves outside the backup store: {rel}"
        )));
    }
    Ok(source)
}

/// `backup_load_targets`: the validated records of `targets.tsv`, in order.
pub fn load_targets(supplied_root: &str, requested: &str) -> Result<Vec<Target>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let snapshot = snapshot_path(supplied_root, requested)?;
    let tsv = format!("{snapshot}/targets.tsv");
    let bytes = std::fs::read(&tsv).map_err(|e| Error::io(&tsv, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut targets = Vec::new();
    for line in text.split('\n') {
        let fields = line.trim_matches('\t');
        let (state, rest) = fields.split_once('\t').unwrap_or((fields, ""));
        let rest = rest.trim_start_matches('\t');
        let (rel, extra) = rest.split_once('\t').unwrap_or((rest, ""));
        let extra = extra.trim_start_matches('\t');
        if state.is_empty() && rel.is_empty() && extra.is_empty() {
            continue;
        }
        if state != "present" && state != "missing" {
            return Err(refuse(format!("Invalid target state in snapshot: {state}")));
        }
        if !extra.is_empty() {
            return Err(refuse(format!("Invalid target record in snapshot: {rel}")));
        }
        validate_rel(rel, false)?;
        let present = state == "present";
        if present {
            let source = snapshot_source(&snapshot, rel)?;
            if !exists_or_link(Path::new(&source)) {
                return Err(refuse(format!(
                    "Snapshot content is missing for target: {rel}"
                )));
            }
        }
        safe_target_path(&canonical, rel, false)?;
        targets.push(Target {
            present,
            rel: rel.to_string(),
        });
    }
    if targets.is_empty() {
        return Err(refuse("Backup snapshot contains no targets"));
    }
    Ok(targets)
}

/// `backup_restore`: every recorded target is removed, then the present ones
/// are copied back.
pub fn restore(supplied_root: &str, requested: &str) -> Result<(), Error> {
    let canonical = canonical_root(supplied_root)?;
    let snapshot = snapshot_path(supplied_root, requested)?;
    let targets = load_targets(supplied_root, &snapshot)?;
    for target in &targets {
        let path = safe_target_path(&canonical, &target.rel, false)?;
        remove_all(Path::new(&path)).map_err(|e| Error::io(&path, e))?;
    }
    for target in targets.iter().filter(|t| t.present) {
        let path = safe_target_path(&canonical, &target.rel, false)?;
        let parent = paths::parent(&path);
        std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
        let source = snapshot_source(&snapshot, &target.rel)?;
        copy_preserving(Path::new(&source), Path::new(&path)).map_err(|e| Error::io(&path, e))?;
    }
    Ok(())
}

fn complete_snapshots(store: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(store) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .map(|name| format!("{store}/{name}"))
        .filter(|path| {
            let path = Path::new(path);
            !path.is_symlink() && path.is_dir() && path.join(".complete").is_file()
        })
        .collect();
    found.sort();
    found
}

/// `backup_latest`.
pub fn latest(supplied_root: &str) -> Result<Option<String>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    if !Path::new(&store).is_dir() {
        return Ok(None);
    }
    let pointer = PathBuf::from(format!("{store}/.latest"));
    if pointer.is_file() && !pointer.is_symlink() {
        let text = std::fs::read(&pointer).unwrap_or_default();
        let id = String::from_utf8_lossy(&text)
            .split('\n')
            .next()
            .unwrap_or("")
            .to_string();
        if !id.is_empty()
            && id == paths::leaf(&id)
            && Path::new(&format!("{store}/{id}/.complete")).is_file()
        {
            return Ok(Some(format!("{store}/{id}")));
        }
    }
    Ok(complete_snapshots(&store).pop())
}

/// `backup_list`: `(id, operation, created_at)` per complete snapshot.
pub fn list(supplied_root: &str) -> Result<Vec<(String, String, String)>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    Ok(complete_snapshots(&store)
        .into_iter()
        .map(|path| {
            let metadata = std::fs::read(format!("{path}/metadata")).unwrap_or_default();
            let metadata = String::from_utf8_lossy(&metadata);
            let field = |key: &str| {
                metadata
                    .split('\n')
                    .find_map(|line| line.strip_prefix(key))
                    .unwrap_or("")
                    .to_string()
            };
            (
                paths::leaf(&path),
                field("operation="),
                field("created_at="),
            )
        })
        .collect())
}

/// `backup_prune`: a snapshot survives when it is among the newest `limit` and
/// at most `max_age` whole UTC days old; 0 disables a bound; the latest stays.
pub fn prune(supplied_root: &str, limit: Option<&str>, max_age: Option<&str>) -> Result<(), Error> {
    prune_at(supplied_root, limit, max_age, SystemTime::now())
}

fn prune_at(
    supplied_root: &str,
    limit: Option<&str>,
    max_age: Option<&str>,
    now: SystemTime,
) -> Result<(), Error> {
    let limit = limit.filter(|v| !v.is_empty()).unwrap_or("10");
    let max_age = max_age.filter(|v| !v.is_empty()).unwrap_or("30");
    let digits = |v: &str| v.bytes().all(|b| b.is_ascii_digit());
    if !digits(limit) {
        return Err(refuse(format!(
            "Backup limit must be a non-negative integer: {limit}"
        )));
    }
    if !digits(max_age) {
        return Err(refuse(format!(
            "Backup max age must be a non-negative integer: {max_age}"
        )));
    }
    let limit: u64 = limit.parse().unwrap_or(u64::MAX);
    let max_age: u64 = max_age.parse().unwrap_or(u64::MAX);

    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    let snapshots = complete_snapshots(&store);
    if snapshots.is_empty() {
        return Ok(());
    }
    let latest = latest(&canonical)?.unwrap_or_default();

    let today = days_since_epoch(now);
    let mut survivors = Vec::new();
    for candidate in snapshots {
        let too_old = max_age > 0
            && candidate != latest
            && snapshot_day(&paths::leaf(&candidate))
                .is_some_and(|day| today - day > max_age as i64);
        if too_old {
            remove_all(Path::new(&candidate)).map_err(|e| Error::io(&candidate, e))?;
        } else {
            survivors.push(candidate);
        }
    }

    if limit == 0 || survivors.len() as u64 <= limit {
        return Ok(());
    }
    let mut remove_count = survivors.len() as u64 - limit;
    for candidate in survivors {
        if remove_count == 0 {
            break;
        }
        if candidate == latest {
            continue;
        }
        remove_all(Path::new(&candidate)).map_err(|e| Error::io(&candidate, e))?;
        remove_count -= 1;
    }
    Ok(())
}

/// `_backup_snapshot_age_days` without the subtraction: the UTC day of an id's
/// `YYYYMMDDTHHMMSSZ-` prefix.
fn snapshot_day(id: &str) -> Option<i64> {
    let bytes = id.as_bytes();
    let shaped = bytes.len() >= 17
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z'
        && bytes[16] == b'-';
    if !shaped {
        return None;
    }
    let number = |range: std::ops::Range<usize>| id[range].parse::<i64>().ok();
    Some(days_from_civil(number(0..4)?, number(4..6)?, number(6..8)?))
}

/// `_backup_days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = if month > 2 {
        (153 * (month - 3) + 2) / 5 + day - 1
    } else {
        (153 * (month + 9) + 2) / 5 + day - 1
    };
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_since_epoch(now: SystemTime) -> i64 {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
}

/// `date -u +%Y%m%dT%H%M%SZ`.
fn utc_stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Project {
        _dir: tempfile::TempDir,
        root: String,
    }

    impl Project {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(dir.path())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            Self { _dir: dir, root }
        }

        fn path(&self, rel: &str) -> PathBuf {
            Path::new(&self.root).join(rel)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.path(rel)).unwrap()
        }

        fn abs(&self, rel: &str) -> String {
            format!("{}/{rel}", self.root)
        }

        fn fake_snapshot(&self, id: &str) {
            self.write(
                &format!(".ai/backups/{id}/metadata"),
                "schema=1\noperation=sync\n",
            );
            self.write(
                &format!(".ai/backups/{id}/targets.tsv"),
                "missing\tAGENTS.md\n",
            );
            self.write(&format!(".ai/backups/{id}/.complete"), "");
            std::fs::create_dir_all(self.path(&format!(".ai/backups/{id}/files"))).unwrap();
        }
    }

    fn at(stamp_secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(stamp_secs)
    }

    #[test]
    fn stamps_and_days_agree_with_date_and_days_from_civil() {
        assert_eq!(utc_stamp(at(0)), "19700101T000000Z");
        assert_eq!(utc_stamp(at(1_789_323_442)), "20260913T181722Z");
        assert_eq!(utc_stamp(at(951_782_400)), "20000229T000000Z");
        assert_eq!(days_from_civil(2020, 1, 1), 18_262);
        assert_eq!(snapshot_day("20200101T000000Z-sync-1"), Some(18_262));
        assert_eq!(snapshot_day("not-a-timestamp"), None);
    }

    #[test]
    fn a_restore_brings_back_existing_targets_and_removes_later_ones() {
        let p = Project::new();
        p.write("AGENTS.md", "before\n");
        p.write(".claude/settings.json", "settings-before\n");
        let snapshot = create_at(
            &p.root,
            "sync",
            &[
                p.abs("AGENTS.md"),
                p.abs(".claude/settings.json"),
                p.abs(".claude/rules"),
            ],
            at(1_789_323_442),
        )
        .unwrap();
        let id = format!("20260913T181722Z-sync-{}", std::process::id());
        assert_eq!(snapshot, p.abs(&format!(".ai/backups/{id}")));
        assert_eq!(
            p.read(&format!(".ai/backups/{id}/targets.tsv")),
            "present\tAGENTS.md\npresent\t.claude/settings.json\nmissing\t.claude/rules\n"
        );
        assert_eq!(
            p.read(&format!(".ai/backups/{id}/metadata")),
            "schema=1\noperation=sync\ncreated_at=20260913T181722Z\n"
        );
        assert_eq!(p.read(".ai/backups/.latest"), format!("{id}\n"));
        assert_eq!(p.read(".ai/backups/.gitignore"), "*\n");

        p.write("AGENTS.md", "after\n");
        p.write(".claude/rules/core.md", "generated\n");
        restore(&p.root, &snapshot).unwrap();
        assert_eq!(p.read("AGENTS.md"), "before\n");
        assert_eq!(p.read(".claude/settings.json"), "settings-before\n");
        assert!(!p.path(".claude/rules").exists());
    }

    #[test]
    fn nested_and_duplicate_targets_collapse_into_the_shallowest_root() {
        let p = Project::new();
        p.write(".amazonq/rules/00-context.md", "context\n");
        let snapshot = create(
            &p.root,
            "sync",
            &[
                p.abs(".amazonq/rules/00-context.md"),
                p.abs(".amazonq/rules"),
                p.abs(".amazonq/rules"),
            ],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(format!("{snapshot}/targets.tsv")).unwrap(),
            "present\t.amazonq/rules\n"
        );
        assert!(Path::new(&format!("{snapshot}/files/.amazonq/rules/00-context.md")).is_file());
    }

    #[test]
    fn the_root_the_store_and_outside_paths_are_refused() {
        let p = Project::new();
        let refused = |target: String| create(&p.root, "sync", &[target]).unwrap_err().to_string();
        assert_eq!(
            refused(p.root.clone()),
            "Refusing to back up the repository root"
        );
        assert_eq!(
            refused(paths::parent(&p.root)),
            format!(
                "Backup target is outside the repository root: {}",
                paths::parent(&p.root)
            )
        );
        assert_eq!(
            refused(p.abs(".ai")),
            "Refusing to back up a parent of the backup store: .ai"
        );
        assert_eq!(
            refused(p.abs(".ai/backups/x")),
            "Refusing to back up the backup store itself: .ai/backups/x"
        );
    }

    #[test]
    fn links_modes_and_times_survive_a_round_trip() {
        use std::os::unix::fs::PermissionsExt;
        let p = Project::new();
        p.write("AGENTS.md", "agents\n");
        std::os::unix::fs::symlink("AGENTS.md", p.path("CLAUDE.md")).unwrap();
        p.write(".claude/hooks/guard.sh", "#!/bin/sh\n");
        let guard = p.path(".claude/hooks/guard.sh");
        std::fs::set_permissions(&guard, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = at(1_600_000_000);
        OpenOptions::new()
            .write(true)
            .open(&guard)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let snapshot = create(&p.root, "sync", &[p.abs("CLAUDE.md"), p.abs(".claude")]).unwrap();
        std::fs::remove_file(p.path("CLAUDE.md")).unwrap();
        std::fs::remove_dir_all(p.path(".claude")).unwrap();
        restore(&p.root, &snapshot).unwrap();

        assert_eq!(
            std::fs::read_link(p.path("CLAUDE.md")).unwrap(),
            Path::new("AGENTS.md")
        );
        let meta = std::fs::metadata(&guard).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        assert_eq!(meta.modified().unwrap(), old);
    }

    #[test]
    fn a_store_or_snapshot_reached_through_a_symlink_is_refused() {
        let p = Project::new();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), p.path(".ai")).unwrap();
        p.write("AGENTS.md", "source\n");
        assert_eq!(
            create(&p.root, "sync", &[p.abs("AGENTS.md")])
                .unwrap_err()
                .to_string(),
            "AgentSync state directory cannot be a symlink: .ai"
        );
        assert!(!outside.path().join("backups").exists());

        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        create(&p.root, "init", &[p.abs("AGENTS.md")]).unwrap();
        let forged = tempfile::tempdir().unwrap();
        std::fs::create_dir(forged.path().join("files")).unwrap();
        std::fs::write(forged.path().join("targets.tsv"), "present\tAGENTS.md\n").unwrap();
        std::fs::write(forged.path().join("files/AGENTS.md"), "outside\n").unwrap();
        std::fs::write(forged.path().join(".complete"), "").unwrap();
        std::os::unix::fs::symlink(forged.path(), p.path(".ai/backups/forged")).unwrap();
        assert_eq!(
            restore(&p.root, "forged").unwrap_err().to_string(),
            "Backup snapshot is missing or incomplete: forged"
        );
        assert_eq!(p.read("AGENTS.md"), "source\n");
    }

    #[test]
    fn metadata_updates_replace_symlinks_instead_of_following_them() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        create(&p.root, "init", &[p.abs("AGENTS.md")]).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("ignore"), "outside-ignore\n").unwrap();
        std::fs::remove_file(p.path(".ai/backups/.gitignore")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("ignore"),
            p.path(".ai/backups/.gitignore"),
        )
        .unwrap();
        create(&p.root, "sync", &[p.abs("AGENTS.md")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.path().join("ignore")).unwrap(),
            "outside-ignore\n"
        );
        assert!(!p.path(".ai/backups/.gitignore").is_symlink());
    }

    #[test]
    fn pruning_bounds_count_and_age_and_always_keeps_the_latest() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.fake_snapshot("not-a-timestamp");
        let now = at(1_789_323_442);
        let first = create_at(&p.root, "init", &[p.abs("AGENTS.md")], now).unwrap();
        let second = create_at(&p.root, "sync", &[p.abs("AGENTS.md")], now).unwrap();
        let newest = create_at(&p.root, "sync", &[p.abs("AGENTS.md")], now).unwrap();
        assert!(newest.ends_with(&format!("-sync-{}-2", std::process::id())));

        prune_at(&p.root, Some("10"), Some("0"), now).unwrap();
        assert!(p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        prune_at(&p.root, None, None, now).unwrap();
        assert!(!p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        assert!(p.path(".ai/backups/not-a-timestamp").exists());

        prune_at(&p.root, Some("2"), Some("30"), now).unwrap();
        assert!(!Path::new(&first).exists());
        assert!(!Path::new(&second).exists());
        assert!(p.path(".ai/backups/not-a-timestamp").exists());
        assert_eq!(latest(&p.root).unwrap(), Some(newest));

        assert_eq!(
            prune_at(&p.root, Some("x"), None, now)
                .unwrap_err()
                .to_string(),
            "Backup limit must be a non-negative integer: x"
        );
    }

    #[test]
    fn the_latest_survives_an_age_limit_and_stale_staging_is_swept() {
        let p = Project::new();
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.write(".ai/backups/.latest", "20200101T000000Z-sync-1\n");
        let now = at(1_789_323_442);
        prune_at(&p.root, Some("10"), Some("1"), now).unwrap();
        assert_eq!(
            latest(&p.root).unwrap(),
            Some(p.abs(".ai/backups/20200101T000000Z-sync-1"))
        );

        p.write("AGENTS.md", "source\n");
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.abandoned/files")).unwrap();
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.inflight/files")).unwrap();
        p.write(".ai/backups/.latest.tmp.stale", "");
        let old = at(1_577_836_800);
        std::fs::File::open(p.path(".ai/backups/.tmp.sync.abandoned"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(p.path(".ai/backups/.latest.tmp.stale"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        create_at(&p.root, "sync", &[p.abs("AGENTS.md")], now).unwrap();
        assert!(!p.path(".ai/backups/.tmp.sync.abandoned").exists());
        assert!(!p.path(".ai/backups/.latest.tmp.stale").exists());
        assert!(p.path(".ai/backups/.tmp.sync.inflight").exists());

        let rows = list(&p.root).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            (
                "20200101T000000Z-sync-1".to_string(),
                "sync".to_string(),
                String::new()
            )
        );
        assert_eq!(rows[1].1, "sync");
        assert_eq!(rows[1].2, "20260913T181722Z");
    }
}
```

- [ ] **Step 2: Add the refusal variant to `src/error.rs`**

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
    /// A refusal of `lib/helpers/backup.sh`, printed as `Error: <message>`.
    #[error("{0}")]
    Backup(String),
    #[error("Repository root not found: {}", .0.display())]
    ProjectRootNotFound(PathBuf),
    #[error(
        "native binary is v{binary} but the engine is v{engine}. Rebuild it: cargo build --release"
    )]
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

- [ ] **Step 3: Declare the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod backup;
pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod file_ops;
pub mod filters;
pub mod gitignore;
pub mod log;
pub mod manifest;
pub mod opencode_json;
pub mod overlay;
pub mod paths;
pub mod payload;
pub mod profiles;
pub mod project;
pub mod render;
pub mod rules;
pub mod session;
pub mod staging;
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

- [ ] **Step 4: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

Expected: `138 passed` (unit) and `7 passed` (integration); fmt and clippy exit 0. The nine `backup::tests` mirror `tests/backup.bats`: restore of present and missing targets, collapse of nested targets, the refusals, links, modes, and times, symlinked store and snapshot, metadata written through symlinks, count and age pruning, the latest surviving an age limit, and the stale-staging sweep.

- [ ] **Step 5: Cross-check the stamp arithmetic**

```bash
date -u -r 1789323442 +%Y%m%dT%H%M%SZ 2>/dev/null || date -u -d @1789323442 +%Y%m%dT%H%M%SZ
date -u -r 951782400 +%Y%m%dT%H%M%SZ 2>/dev/null || date -u -d @951782400 +%Y%m%dT%H%M%SZ
bash -c 'source lib/helpers/backup.sh; _backup_days_from_civil 2020 1 1'
```

Expected: `20260913T181722Z`, `20000229T000000Z`, `18262` — the values `stamps_and_days_agree_with_date_and_days_from_civil` asserts.

- [ ] **Step 6: Commit**

```bash
git add src/backup.rs src/error.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): create, restore, and prune backups in the Bash layout"
```

---

### Task 6: The Sync Transaction

**Files:**
- Modify: `src/render.rs` (whole file)
- Modify: `src/overlay.rs` (`setup_shared`, `setup_base_src`, two tests)
- Modify: `src/paths.rs` (`ai_dir_enclosing_root`, one test)
- Create: `src/cli/sync.rs`
- Modify: `src/cli/mod.rs` (whole file)
- Modify: `src/main.rs` (whole file)
- Modify: `tests/cli.rs` (three integration tests)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces: in `render` — `Env { config_path, skip_post_sync, allow_post_sync }` (raw environment values; `check` passes `skip_post_sync: Some("true")`); `Selection { only, skip, profile }` with `Selection::includes(&self, slug: &str) -> bool` (`should_sync_tool`); `Run`, whose public fields `config`, `config_path`, `update_gitignore`, `outputs`, `sources`, `backup_targets`, `gitignore_generated`, `gitignore_profile`, `synced`, `skipped`, `total`, `skipped_names` the transaction reads; the stages `prepare(s, &Env, Selection) -> Result<Run, Stop>`, `check_version_pin(s, &Run) -> Step`, `banner(s)`, `setup_overlays(s, &mut Run, shared: bool) -> Step`, `build_catalog(s, &mut Run)`, `run_passes(s, &mut Run) -> Step`; `render(s, &Env) -> Step` composes them for `check`; `pub const TARGET_KEYS` and `pub type Step`. `overlay::setup_shared(s, config: &str, sources: &mut Sources) -> Result<Option<String>, Error>` (`shared_setup_overlay` with its warnings) and `overlay::setup_base_src(s, config: Option<&str>, child_src: &str, sources: &mut Sources)`. `paths::ai_dir_enclosing_root(dir: &str) -> Option<String>`. In `cli::sync` — `USAGE`; `Args`; `Parsed { Run(Args), Help, Invalid(String) }`; `parse(args: &[String]) -> Parsed` (`parse_args`); `Env { render: render::Env, skip_backup: bool, backup_limit: Option<String>, backup_max_age: Option<String> }`; `run(root: &str, args: &[String], env: &Env, colors: bool, sink: Sink) -> u8`. `Command::Sync { args: Vec<String> }`. The binary answers `agentsync sync` when called directly; `bin/agentsync.sh` does not delegate to it until Task 7.

- [ ] **Step 1: Replace `src/render.rs`**

The personal and profile passes move out of `render` into stages, the dests collection also records backup targets and the `.gitignore` payload, `sync_tool` and `cleanup_tool` keep the run's counts and skipped names, and `run_post_sync_hook` runs a trusted hook with `bash -lc` in the project root. The step functions from `sync_rules_step` to `compose_opencode` are unchanged from Task 2.

```rust
//! The run of `lib/sync.sh`: config and sources, the `shared:` and engine skill
//! overlays, every enabled tool and selected profile rendered into the
//! workspace, disabled tools cleaned. `check` renders forced and in memory;
//! `cli::sync` runs the stages on disk with its transaction between them.

use std::collections::BTreeSet;

use crate::overlay::{self, Sources};
use crate::rules::{self, Conversion, RuleOptions};
use crate::session::Session;
use crate::tool::Tool;
use crate::{
    Error, catalog, engine_version, file_ops, opencode_json, paths, payload, profiles, yaml_subset,
};

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

/// Environment `sync.sh` reads. `check` sets `skip_post_sync`, as
/// `lib/check.sh` exported `AGENTSYNC_SKIP_POST_SYNC=true`.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
    pub skip_post_sync: Option<String>,
    pub allow_post_sync: Option<String>,
}

/// `--only`, `--skip`, and `--profile`.
#[derive(Default)]
pub struct Selection {
    pub only: String,
    pub skip: String,
    pub profile: Option<String>,
}

impl Selection {
    /// `should_sync_tool`.
    pub fn includes(&self, slug: &str) -> bool {
        let listed = |csv: &str| format!(",{csv},").contains(&format!(",{slug},"));
        (self.only.is_empty() || listed(&self.only))
            && (self.skip.is_empty() || !listed(&self.skip))
    }
}

/// The globals `sync.sh` fills as it goes.
pub struct Run {
    pub config: Option<String>,
    pub config_path: Option<String>,
    cleanup: String,
    pub update_gitignore: bool,
    pub outputs: &'static str,
    skip_post_sync: bool,
    allow_post_sync: bool,
    pub sources: Sources,
    base_sources: Sources,
    profile_base_src: String,
    selection: Selection,
    profiles: Vec<String>,
    enabled: BTreeSet<String>,
    profile_tools: BTreeSet<String>,
    protected: Vec<String>,
    pub backup_targets: Vec<String>,
    pub gitignore_generated: Vec<String>,
    pub gitignore_profile: Vec<String>,
    tools: Vec<String>,
    printed: bool,
    pub synced: usize,
    pub skipped: usize,
    pub total: usize,
    pub skipped_names: Vec<String>,
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

/// What `check` needs: `sync.sh --force` without its transaction and without
/// `shared:`, which `lib/check.sh` merged into the workspace beforehand.
pub fn render(s: &mut Session, env: &Env) -> Step {
    let mut run = prepare(s, env, Selection::default())?;
    check_version_pin(s, &run)?;
    banner(s);
    setup_overlays(s, &mut run, false)?;
    build_catalog(s, &mut run);
    run_passes(s, &mut run)
}

/// `_load_run_config` and `_resolve_sources`.
pub fn prepare(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
    let mut run = load_run_config(s, env, selection)?;
    resolve_sources(s, &mut run)?;
    Ok(run)
}

/// `resolve_project_config_path` and `_load_run_config`.
fn load_run_config(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
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

    fn env_set(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|v| !v.is_empty())
    }
    let allow_post_sync = match env_set(&env.allow_post_sync) {
        Some(value) => value == "true",
        None => yaml_subset::value(catalog::GLOBAL_CONFIG, "post_sync.allow") == "true",
    };
    let mut skip_post_sync = env_set(&env.skip_post_sync) == Some("true");
    let mut cleanup = "true".to_string();
    let mut update_gitignore = true;
    let mut outputs = "local";
    if let (Some(text), Some(path)) = (&config, &config_path) {
        let configured = yaml_subset::value(text, "defaults.cleanup");
        if !configured.is_empty() {
            cleanup = configured;
        }
        if env_set(&env.skip_post_sync).is_none()
            && yaml_subset::value(text, "post_sync.skip") == "true"
        {
            skip_post_sync = true;
        }
        update_gitignore = yaml_subset::value(text, "gitignore.update") != "false";
        outputs = match yaml_subset::value(text, "outputs")
            .replace('"', "")
            .as_str()
        {
            "committed" => "committed",
            "local" => "local",
            "" if !update_gitignore => "committed",
            "" => "local",
            other => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown outputs mode '{other}' in {shown} — expected 'committed' or 'local'"
                ));
                return Err(Stop(1));
            }
        };
    }

    Ok(Run {
        config,
        config_path,
        cleanup,
        update_gitignore,
        outputs,
        skip_post_sync,
        allow_post_sync,
        sources: Sources::default(),
        base_sources: Sources::default(),
        profile_base_src: String::new(),
        selection,
        profiles: Vec::new(),
        enabled: BTreeSet::new(),
        profile_tools: BTreeSet::new(),
        protected: Vec::new(),
        backup_targets: Vec::new(),
        gitignore_generated: Vec::new(),
        gitignore_profile: Vec::new(),
        tools: Vec::new(),
        printed: false,
        synced: 0,
        skipped: 0,
        total: 0,
        skipped_names: Vec::new(),
    })
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
pub fn check_version_pin(s: &mut Session, run: &Run) -> Step {
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
    if run.outputs == "committed" {
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

/// `_print_banner`.
pub fn banner(s: &mut Session) {
    s.log.separator();
    if s.dry_run {
        s.log.info("Starting AgentSync Config Sync (DRY RUN)...");
    } else {
        s.log.info("Starting AgentSync Config Sync...");
    }
    s.log.separator();
    s.log.out(String::new());
}

/// `shared_setup_overlay` when `shared` is set, `base_src_setup_overlay`, and
/// `_snapshot_base_sources`.
pub fn setup_overlays(s: &mut Session, run: &mut Run, shared: bool) -> Step {
    let config = run.config.clone();
    let mut child_src = format!("{}/.ai/src", s.paths.root);
    if shared
        && let Some(text) = config.as_deref()
        && let Some(dir) = overlay::setup_shared(s, text, &mut run.sources).map_err(|e| io(s, e))?
    {
        child_src = format!("{dir}/src");
    }
    overlay::setup_base_src(s, config.as_deref(), &child_src, &mut run.sources)
        .map_err(|e| io(s, e))?;
    run.base_sources = run.sources.clone();
    run.profile_base_src = child_src;
    Ok(())
}

/// `_build_tool_catalog`, the `warm_*_cache` sets, and `_collect_protected_dests`.
pub fn build_catalog(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    run.profiles = match &run.selection.profile {
        Some(name) => vec![name.clone()],
        None => profiles::names(&text)
            .into_iter()
            .filter(|name| profiles::is_active(&text, name))
            .collect(),
    };
    load_tools(s, run);
    collect_protected_dests(s, run);
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
/// any profile tool claims; the transaction snapshots every dest the run may
/// change; `.gitignore` gets the dests of enabled and profile tools.
fn collect_protected_dests(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    let selected_profile_tools: BTreeSet<String> = run
        .profiles
        .iter()
        .flat_map(|name| profiles::tools(&text, name))
        .collect();

    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        let tool = load_tool(s, slug);
        if run.enabled.contains(slug) {
            let dests = collect_tool_dests(s, run, &tool, false);
            if run.selection.includes(slug) {
                run.backup_targets.extend(dests);
            }
        } else if run.cleanup == "true" {
            for key in TARGET_KEYS {
                let raw = tool.value(&format!("targets.{key}.dest"));
                if raw.is_empty() {
                    continue;
                }
                let label = format!("targets.{key}.dest for {slug}");
                if let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) {
                    run.backup_targets.push(abs);
                }
            }
        }
    }
    for slug in run.profile_tools.clone() {
        let tool = load_tool(s, &slug);
        let dests = collect_tool_dests(s, run, &tool, true);
        if selected_profile_tools.contains(&slug) && run.selection.includes(&slug) {
            run.backup_targets.extend(dests);
        }
    }
}

/// `_collect_tool_dests`: the tool's resolved dests, also recorded for cleanup
/// protection and the `.gitignore` payload.
fn collect_tool_dests(s: &mut Session, run: &mut Run, tool: &Tool, profile: bool) -> Vec<String> {
    let mut collected = Vec::new();
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {}", tool.slug);
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        run.protected.push(abs.clone());
        collected.push(abs.clone());
        let Some(mut rel) = s.paths.to_repo_relative(&abs) else {
            s.log
                .error(&format!("Path is outside repository root: {abs}"));
            continue;
        };
        if matches!(key, "rules" | "skills" | "commands" | "subagents") {
            rel.push('/');
        }
        if profile && tool.flag(&format!("targets.{key}.profile_scoped")) != Some(false) {
            run.gitignore_profile.push(rel);
        } else {
            run.gitignore_generated.push(rel);
        }
    }
    collected
}

/// `_run_personal_pass` and `_run_profile_passes`.
pub fn run_passes(s: &mut Session, run: &mut Run) -> Step {
    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        run.total += 1;
        if run.enabled.contains(slug) {
            sync_tool(s, run, slug)?;
        } else {
            cleanup_tool(s, run, slug);
        }
        if run.printed {
            s.log.out(String::new());
        }
    }

    let text = run.config.clone().unwrap_or_default();
    for profile in run.profiles.clone() {
        let tools: Vec<String> = profiles::tools(&text, &profile)
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        if tools.is_empty() {
            continue;
        }
        s.log.separator();
        s.log.info(&format!("Profile: {profile}"));
        run.sources = run.base_sources.clone();
        let base_src = run.profile_base_src.clone();
        overlay::setup_profile(s, &text, &profile, &base_src, &mut run.sources)
            .map_err(|e| io(s, e))?;
        for slug in tools {
            run.total += 1;
            sync_tool(s, run, &slug)?;
            if run.printed {
                s.log.out(String::new());
            }
        }
        overlay::cleanup_profile(&mut s.ws).map_err(|e| io(s, e))?;
    }
    Ok(())
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
    if !run.selection.includes(slug) {
        run.skipped_names.push(display);
        run.skipped += 1;
        run.printed = false;
        return Ok(());
    }
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

    let post_sync = tool.value("post_sync");
    if !s.dry_run && !run_post_sync_hook(s, run, &display, &post_sync) {
        s.log.error(&format!(
            "Sync failed because post-sync hook failed for {display}"
        ));
        return Err(Stop(1));
    }
    s.log.success(&format!("{display} complete"));
    run.synced += 1;
    Ok(())
}

/// `run_post_sync_hook`: false when the hook ran and failed.
fn run_post_sync_hook(s: &mut Session, run: &Run, display: &str, command: &str) -> bool {
    if command.is_empty() {
        return true;
    }
    if run.skip_post_sync {
        s.log.info(&format!(
            "Skipping post-sync hook for {display} (AGENTSYNC_SKIP_POST_SYNC=true)"
        ));
        return true;
    }
    if !run.allow_post_sync {
        s.log.warning(&format!(
            "Skipping post-sync hook for {display} (set AGENTSYNC_ALLOW_POST_SYNC=true to enable)"
        ));
        return true;
    }
    s.log.info(&format!("Running post-sync hook: {command}"));
    let succeeded = std::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(&s.paths.root)
        .status()
        .is_ok_and(|status| status.success());
    if !succeeded {
        s.log.warning("Post-sync hook failed");
    }
    succeeded
}

fn sync_rules_step(s: &mut Session, run: &Run, tool: &Tool, dests: &Dests, display: &str) -> Step {
    let src_agents = tool_source(s, tool, "agents", &run.sources.agents, display)?;
    let src_rules = tool_source(s, tool, "rules", &run.sources.rules, display)?;
    let include = tool.filter("targets.rules.include");
    let exclude = tool.filter("targets.rules.exclude");

    if tool.value("targets.rules.inline_into_agents") == "true" && !dests.agents.is_empty() {
        if s.dry_run {
            s.log.step(&format!(
                "Would append rule references to {} (dry-run)",
                paths::leaf(&dests.agents)
            ));
        } else {
            inline_rules_into_agents(s, &src_rules, &dests.agents, &include, &exclude)?;
        }
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
            if tool.value("targets.rules.append_imports") == "true" && !s.dry_run {
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
        && !s.dry_run
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
        if !target.is_empty() && !s.dry_run {
            inline_skills_into_file(s, &src_skills, &target, &include, &exclude)?;
        } else if s.dry_run {
            s.log.step("Would append skill index (dry-run)");
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
        if !target.is_empty() && s.dry_run {
            s.log.step("Would append command index (dry-run)");
        } else if !target.is_empty() {
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
            if resource == "guard" && !s.dry_run {
                s.ws.make_executable(dest).map_err(|e| io(s, e))?;
            }
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
        Ok(_) if s.dry_run => {
            s.log.step(&format!(
                "Would compose OpenCode settings and MCP → {} (dry-run)",
                s.display(dest)
            ));
            Ok(())
        }
        Ok(composed) => {
            s.ws.create_dir_all(&paths::parent(dest))
                .map_err(|e| io(s, e))?;
            s.ws.remove(dest).map_err(|e| io(s, e))?;
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
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    run.skipped_names.push(display.clone());
    run.skipped += 1;
    run.printed = false;
    if run.cleanup != "true" {
        return;
    }
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

- [ ] **Step 2: The `shared:` overlay for `sync` in `src/overlay.rs`**

Replace `setup_base_src` and its doc comment with `setup_shared` followed by the new `setup_base_src`:

```rust
/// `shared_setup_overlay`: the parent's files of the inherited categories fill
/// what the project lacks. Returns the overlay directory when one was built.
pub fn setup_shared(
    s: &mut Session,
    config: &str,
    sources: &mut Sources,
) -> Result<Option<String>, Error> {
    let raw_path = yaml_subset::value(config, "shared.path");
    let raw_inherit = yaml_subset::value(config, "shared.inherit");
    if raw_path.is_empty() || raw_inherit.is_empty() {
        return Ok(None);
    }
    let root = s.paths.root.clone();
    let parent_root = if raw_path.starts_with('/') {
        raw_path.clone()
    } else {
        format!("{root}/{raw_path}")
    };
    if !Path::new(&parent_root).is_dir() {
        s.log.warning(&format!(
            "shared.path does not exist: {raw_path} — overlay skipped"
        ));
        return Ok(None);
    }
    let parent_root = paths::normalize(&parent_root);
    let nested = format!("{parent_root}/.ai/src");
    let parent_src = if Path::new(&nested).is_dir() {
        nested
    } else if paths::leaf(&parent_root) == "src" {
        parent_root
    } else {
        s.log.warning(&format!(
            "shared.path has no .ai/src/: {raw_path} — overlay skipped"
        ));
        return Ok(None);
    };
    if parent_src == format!("{root}/.ai/src") {
        s.log
            .warning("shared.path resolves to this project — overlay skipped");
        return Ok(None);
    }

    let mut categories: Vec<&str> = Vec::new();
    for token in raw_inherit
        .split([',', ' ', '\t', '\n'])
        .filter(|t| !t.is_empty())
    {
        match token {
            "subagents" | "agents" => categories.push("agents"),
            "rules" => categories.push("rules"),
            "skills" => categories.push("skills"),
            "commands" => categories.push("commands"),
            unknown => s.log.warning(&format!(
                "shared.inherit: unknown category '{unknown}' — skipped"
            )),
        }
    }
    if categories.is_empty() {
        return Ok(None);
    }
    let child_src = format!("{root}/.ai/src");
    let dir = build_tree(&mut s.ws, "shared", &child_src, &parent_src, &categories)?;
    rewrite_sources(&s.ws, &dir, sources);
    s.log.info(&format!(
        "Shared overlay active: {parent_src} ({})",
        categories.join(",")
    ));
    Ok(Some(dir))
}
```

```rust
/// `base_src_setup_overlay`: engine-owned skills fill paths the project, and a
/// `shared:` parent composed into `child_src`, lack, unless `base_skills: false`.
pub fn setup_base_src(
    s: &mut Session,
    config: Option<&str>,
    child_src: &str,
    sources: &mut Sources,
) -> Result<(), Error> {
    let base_src = format!("{ENGINE_ROOT}/lib/templates/base-src");
    if !s.ws.is_dir(&format!("{base_src}/skills")) {
        return Ok(());
    }
    if config.is_some_and(|text| yaml_subset::value(text, "base_skills") == "false") {
        return Ok(());
    }
    if !s.ws.is_dir(child_src) {
        return Ok(());
    }
    let dir = build_tree(&mut s.ws, "base-src", child_src, &base_src, &["skills"])?;
    rewrite_sources(&s.ws, &dir, sources);
    Ok(())
}
```

In the `tests` module, replace `fn the_base_skill_layer_is_skipped_by_base_skills_false` with:

```rust
    #[test]
    fn the_base_skill_layer_is_skipped_by_base_skills_false() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "a");
        let mut sources = Sources::default();
        setup_base_src(
            &mut s,
            Some("base_skills: false\n"),
            "/proj/.ai/src",
            &mut sources,
        )
        .unwrap();
        assert_eq!(sources, Sources::default());
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();
        assert_eq!(sources.skills, "/<agentsync-overlay>/base-src/src/skills");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/agentsync/SKILL.md"));
    }
```

and add after it:

```rust
    #[cfg(unix)]
    #[test]
    fn a_shared_parent_fills_the_inherited_categories_and_explains_every_skip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("child").to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.path().join("child/.ai/src/rules")).unwrap();
        std::fs::write(dir.path().join("child/.ai/src/AGENTS.md"), "child").unwrap();
        std::fs::create_dir_all(dir.path().join("parent/.ai/src/skills/p")).unwrap();
        std::fs::write(dir.path().join("parent/.ai/src/skills/p/SKILL.md"), "p").unwrap();
        let mut s = Session::new(
            Workspace::on_disk(&root),
            crate::paths::Paths::on_disk(&root),
        );
        let mut sources = Sources::default();

        let config = "shared:\n  path: \"../parent\"\n  inherit: skills, tools\n";
        let overlay = setup_shared(&mut s, config, &mut sources).unwrap();
        assert_eq!(overlay.as_deref(), Some("/<agentsync-overlay>/shared"));
        assert_eq!(sources.skills, "/<agentsync-overlay>/shared/src/skills");
        assert_eq!(sources.agents, "/<agentsync-overlay>/shared/src/AGENTS.md");
        assert!(s.ws.is_file("/<agentsync-overlay>/shared/src/skills/p/SKILL.md"));
        let parent = format!("{}/parent/.ai/src", dir.path().to_string_lossy());
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] shared.inherit: unknown category 'tools' — skipped".to_string(),
                format!("[INFO] Shared overlay active: {parent} (skills)"),
            ]
        );

        for (config, line) in [
            (
                "shared:\n  path: missing\n  inherit: rules\n",
                "[WARNING] shared.path does not exist: missing — overlay skipped",
            ),
            (
                "shared:\n  path: \"..\"\n  inherit: rules\n",
                "[WARNING] shared.path has no .ai/src/: .. — overlay skipped",
            ),
            (
                "shared:\n  path: \".\"\n  inherit: rules\n",
                "[WARNING] shared.path resolves to this project — overlay skipped",
            ),
        ] {
            assert_eq!(setup_shared(&mut s, config, &mut sources).unwrap(), None);
            assert_eq!(s.log.tail(1), [line]);
        }
    }
```

- [ ] **Step 3: `ai_dir_enclosing_root` in `src/paths.rs`**

Add before `#[derive(Clone, Debug)] pub struct Paths`:

```rust
/// `ai_dir_enclosing_root`: the parent of the shallowest `.ai` segment of a
/// logical directory path, when there is one.
pub fn ai_dir_enclosing_root(dir: &str) -> Option<String> {
    let mut shallowest = None;
    let mut current = dir.to_string();
    while current != "/" && !current.is_empty() {
        if leaf(&current) == ".ai" {
            shallowest = Some(current.clone());
        }
        let up = parent(&current);
        if up == current {
            break;
        }
        current = up;
    }
    shallowest.map(|ai| parent(&ai))
}
```

Add this test in the `tests` module, before `fn normalisation_collapses_dot_segments_lexically`:

```rust
    #[test]
    fn a_directory_inside_an_ai_tree_names_the_project_above_its_shallowest_ai() {
        assert_eq!(
            ai_dir_enclosing_root("/p/.ai/src/.ai/x").as_deref(),
            Some("/p")
        );
        assert_eq!(ai_dir_enclosing_root("/p/.ai").as_deref(), Some("/p"));
        assert_eq!(ai_dir_enclosing_root("/.ai").as_deref(), Some("/"));
        assert_eq!(ai_dir_enclosing_root("/p/.aix"), None);
    }
```

- [ ] **Step 4: Create `src/cli/sync.rs`**

```rust
//! `agentsync sync`: `lib/sync.sh` with its transaction. The render writes the
//! project in place, as Bash does; a failure after the backup restores it.

use std::path::Path;

use crate::log::{Log, Sink};
use crate::manifest::{self, Manifest};
use crate::paths::{self, Paths};
use crate::render::{self, Run, Selection, Stop};
use crate::session::Session;
use crate::workspace::Workspace;
use crate::{Error, backup, gitignore};

pub const USAGE: &str = "AgentSync Config Sync Script

Usage: sync.sh [OPTIONS]

Real sync runs snapshot every destination they may change and automatically
restore that snapshot if the run fails. Use 'agentsync rollback' to restore a
successful run manually.

Options:
  --only <tools>    Sync only specified tools (comma-separated)
  --skip <tools>    Skip specified tools (comma-separated)
  --profile <name>  Also sync this profile (default: personal + active profiles)
  --dry-run         Show what would be copied without making changes
  --force           Overwrite destination files even if they were edited manually
  --if-stale        Sync only when source changed since the last sync (else no-op)
  --help            Show this help message
";

/// The options `parse_args` accepts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub dry_run: bool,
    pub force: bool,
    pub if_stale: bool,
    pub only: String,
    pub skip: String,
    pub profile: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Args),
    Help,
    Invalid(String),
}

/// `parse_args`.
pub fn parse(args: &[String]) -> Parsed {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |message: &str| match rest.clone().next() {
            Some(next) if !next.starts_with("--") => {
                rest.next();
                Ok(next.clone())
            }
            _ => Err(Parsed::Invalid(message.to_string())),
        };
        match arg.as_str() {
            "--only" => match value("Option --only requires a comma-separated value") {
                Ok(v) => parsed.only = v,
                Err(invalid) => return invalid,
            },
            "--skip" => match value("Option --skip requires a comma-separated value") {
                Ok(v) => parsed.skip = v,
                Err(invalid) => return invalid,
            },
            "--profile" => match value("Option --profile requires a profile name") {
                Ok(v) => parsed.profile = Some(v),
                Err(invalid) => return invalid,
            },
            "--dry-run" => parsed.dry_run = true,
            "--force" => parsed.force = true,
            "--if-stale" => parsed.if_stale = true,
            "--help" | "-h" => return Parsed::Help,
            other => return Parsed::Invalid(format!("Unknown option: {other}")),
        }
    }
    Parsed::Run(parsed)
}

/// The environment `sync.sh` and `backup.sh` read.
#[derive(Default)]
pub struct Env {
    pub render: render::Env,
    pub skip_backup: bool,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}

/// Runs `sync` for the project at `root`, streaming its log to `sink`, and
/// returns the exit status.
pub fn run(root: &str, args: &[String], env: &Env, colors: bool, sink: Sink) -> u8 {
    let mut log = Log::streaming(colors, sink);
    if let Some(project) = paths::ai_dir_enclosing_root(root) {
        log.error(&format!(
            "Refusing to sync from inside the .ai/ directory: {root}"
        ));
        log.info("Run agentsync from the project root (the parent of .ai/):");
        log.info(&format!("  cd \"{project}\" && agentsync sync"));
        return 2;
    }
    let args = match parse(args) {
        Parsed::Run(args) => args,
        Parsed::Help => {
            print_usage(&mut log);
            return 0;
        }
        Parsed::Invalid(message) => {
            log.error(&message);
            print_usage(&mut log);
            return 1;
        }
    };

    let mut s = Session::new(Workspace::on_disk(root), Paths::on_disk(root));
    s.log = log;
    s.dry_run = args.dry_run;
    s.force = args.force;
    let mut tx = Transaction::default();
    match sync(&mut s, &args, env, &mut tx) {
        Ok(()) => 0,
        Err(Stop(status)) => {
            tx.fail(&mut s, env);
            status
        }
    }
}

fn print_usage(log: &mut Log) {
    for line in USAGE.lines() {
        log.out(line.to_string());
    }
}

/// `SYNC_BACKUP_PATH` while `SYNC_TRANSACTION_ACTIVE`.
#[derive(Default)]
struct Transaction {
    backup: Option<String>,
    active: bool,
}

impl Transaction {
    /// `_sync_cleanup` after a failed run.
    fn fail(&mut self, s: &mut Session, env: &Env) {
        let Some(backup_path) = self.backup.clone().filter(|_| self.active) else {
            return;
        };
        self.active = false;
        let root = s.paths.root.clone();
        let shown = s.display(&backup_path);
        s.log.warning("Sync failed; restoring pre-sync state...");
        match backup::restore(&root, &backup_path) {
            Ok(()) => {
                s.log.info(&format!("Restored pre-sync state from {shown}"));
                prune(s, env);
            }
            Err(e) => {
                report_backup_error(&mut s.log, &e);
                s.log.error(&format!(
                    "Automatic restore failed. Backup retained at {shown}"
                ));
            }
        }
    }
}

fn report_backup_error(log: &mut Log, error: &Error) {
    match error {
        Error::Backup(message) => log.err(format!("Error: {message}")),
        other => log.err(other.to_string()),
    }
}

fn prune(s: &mut Session, env: &Env) {
    let root = s.paths.root.clone();
    if let Err(e) = backup::prune(
        &root,
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
    ) {
        report_backup_error(&mut s.log, &e);
        s.log.warning("Could not prune old AgentSync backups.");
    }
}

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

/// `main` of `lib/sync.sh` after `parse_args`.
fn sync(s: &mut Session, args: &Args, env: &Env, tx: &mut Transaction) -> Result<(), Stop> {
    let selection = Selection {
        only: args.only.clone(),
        skip: args.skip.clone(),
        profile: args.profile.clone(),
    };
    let mut run = render::prepare(s, &env.render, selection)?;
    if args.if_stale && !is_stale(&s.paths.root, &run) {
        return Ok(());
    }
    render::check_version_pin(s, &run)?;
    render::banner(s);
    render::setup_overlays(s, &mut run, true)?;
    render::build_catalog(s, &mut run);

    let root = s.paths.root.clone();
    let previous = Manifest::load(&root).map_err(|e| io(s, e))?;
    s.activate_manifest(previous.as_ref().map(Manifest::paths).unwrap_or_default());
    warn_baseline_replacements(s, &run, previous.is_none());
    check_drift(s, previous.as_ref())?;
    start_transaction(s, &run, env, tx)?;
    render::run_passes(s, &mut run)?;
    finalize(s, &run, previous.as_ref(), tx)?;
    if tx.backup.is_some() {
        prune(s, env);
    }
    tx.active = false;
    Ok(())
}

/// `_sync_is_stale`: no manifest, or any source input modified after it.
fn is_stale(root: &str, run: &Run) -> bool {
    let manifest = Path::new(root).join(manifest::REL);
    let Some(since) = std::fs::metadata(&manifest)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|meta| meta.modified().ok())
    else {
        return true;
    };
    let src = format!("{root}/.ai/src");
    let mut roots: Vec<String> = Vec::new();
    if Path::new(&src).is_dir() {
        roots.push(src.clone());
    }
    let profiles_dir = format!("{root}/.ai/profiles");
    if Path::new(&profiles_dir).is_dir() {
        roots.push(profiles_dir);
    }
    if let Some(config) = run.config_path.as_ref().filter(|p| Path::new(p).is_file()) {
        roots.push(config.clone());
    }
    let sources = &run.sources;
    for rel in [
        &sources.agents,
        &sources.rules,
        &sources.skills,
        &sources.commands,
        &sources.subagents,
    ] {
        if rel.is_empty() {
            continue;
        }
        let abs = if rel.starts_with('/') {
            rel.clone()
        } else {
            format!("{root}/{rel}")
        };
        if paths::is_within(&abs, &src) {
            continue;
        }
        if Path::new(&abs).exists() {
            roots.push(abs);
        }
    }
    if roots.is_empty() {
        return true;
    }
    roots.iter().any(|path| any_newer(Path::new(path), since))
}

/// `find <path> -newer <manifest>`: the path or anything below it, links
/// judged by their own time and never followed.
fn any_newer(path: &Path, since: std::time::SystemTime) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.modified().is_ok_and(|modified| modified > since) {
        return true;
    }
    meta.is_dir()
        && std::fs::read_dir(path).is_ok_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|entry| any_newer(&entry.path(), since))
        })
}

/// `_warn_baseline_replacements`.
fn warn_baseline_replacements(s: &mut Session, run: &Run, baseline: bool) {
    if s.dry_run || !baseline {
        return;
    }
    let mut existing = std::collections::BTreeSet::new();
    for abs in &run.backup_targets {
        let Some(rel) = s.paths.to_repo_relative(abs) else {
            continue;
        };
        let path = Path::new(abs);
        if path.is_file() {
            existing.insert(rel);
        } else if path.is_dir() && has_regular_file(path) {
            existing.insert(format!("{rel}/"));
        }
    }
    if existing.is_empty() {
        return;
    }
    s.log.warning(&format!(
        "First sync in this project — regenerating {} path(s) that already exist:",
        existing.len()
    ));
    for rel in existing {
        s.log.err(format!("      {rel}"));
    }
    s.log
        .err("      Content AgentSync did not generate is replaced from .ai/src/.".into());
    s.log.err(
        "      To keep a file instead, restore it with 'agentsync rollback' and run 'agentsync adopt <file>' first."
            .into(),
    );
}

/// `find <dir> -type f | head -n 1` is not empty.
fn has_regular_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.is_file() {
        return true;
    }
    meta.is_dir()
        && std::fs::read_dir(path).is_ok_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|entry| has_regular_file(&entry.path()))
        })
}

/// `_check_drift_or_exit`.
fn check_drift(s: &mut Session, previous: Option<&Manifest>) -> Result<(), Stop> {
    if s.dry_run {
        return Ok(());
    }
    let drift = previous.map(|m| m.drift(&s.paths.root)).unwrap_or_default();
    if drift.is_empty() {
        return Ok(());
    }
    if s.force {
        s.log.warning(&format!(
            "Overwriting {} file(s) with manual edits (--force):",
            drift.len()
        ));
        for rel in drift {
            s.log.err(format!("      {rel}"));
        }
        return Ok(());
    }
    s.log.error(&format!(
        "Manual edits detected in {} destination file(s) since last sync:",
        drift.len()
    ));
    for rel in drift {
        s.log.err(format!("      {rel}"));
    }
    for line in [
        "",
        "  These files would be silently overwritten. Choose one:",
        "    • Move your edits into .ai/src/, then re-run sync",
        "    • If a tool wrote here out of band, run 'agentsync adopt <file>' to pull it into .ai/src/",
        "    • Re-run with --force to discard the edits and rewrite from source",
        "",
    ] {
        s.log.err(line.to_string());
    }
    Err(Stop(1))
}

/// `_start_sync_transaction`.
fn start_transaction(
    s: &mut Session,
    run: &Run,
    env: &Env,
    tx: &mut Transaction,
) -> Result<(), Stop> {
    if s.dry_run || env.skip_backup {
        return Ok(());
    }
    let root = s.paths.root.clone();
    let mut targets = run.backup_targets.clone();
    if run.update_gitignore {
        targets.push(format!("{root}/.gitignore"));
    }
    targets.push(format!("{root}/{}", manifest::REL));
    match backup::create(&root, "sync", &targets) {
        Ok(path) => {
            tx.backup = Some(path);
            tx.active = true;
            Ok(())
        }
        Err(e) => {
            report_backup_error(&mut s.log, &e);
            s.log
                .error("Could not back up sync targets; no files were changed.");
            Err(Stop(1))
        }
    }
}

/// `_finalize_run`.
fn finalize(
    s: &mut Session,
    run: &Run,
    previous: Option<&Manifest>,
    tx: &Transaction,
) -> Result<(), Stop> {
    let root = s.paths.root.clone();
    if !s.dry_run && run.update_gitignore {
        let mut ignored = run.gitignore_profile.clone();
        if run.outputs != "committed" {
            ignored.extend(run.gitignore_generated.iter().cloned());
            ignored.push(manifest::REL.to_string());
        }
        let path = Path::new(&root).join(".gitignore");
        if !ignored.is_empty() || gitignore::has_managed_block(&path) {
            s.log.separator();
            s.log.info("Updating .gitignore...");
            gitignore::update(&path, &ignored, &mut s.log).map_err(|e| io(s, e))?;
        }
    }
    if !s.dry_run {
        let touched = s.touched().clone();
        manifest::write(&root, previous, &touched, &mut s.log).map_err(|e| io(s, e))?;
    }

    s.log.separator();
    if s.preserved() > 0 {
        s.log.warning(&format!(
            "Preserved {} user-added file(s) not in .ai/src/ — move them into .ai/src/ to manage them, or re-run with --force to prune.",
            s.preserved()
        ));
    }
    if !run.skipped_names.is_empty() {
        s.log
            .info(&format!("Skipped: {}", run.skipped_names.join(", ")));
    }
    let mut summary = format!("Synced {}/{} tools", run.synced, run.total);
    if run.skipped > 0 {
        summary.push_str(&format!(" ({} skipped)", run.skipped));
    }
    if s.dry_run {
        summary.push_str(" (dry-run)");
    }
    if let Some(path) = &tx.backup {
        let shown = s.display(path);
        s.log.info(&format!("Backup: {shown}"));
    }
    s.log.done(&summary);
    s.log.separator();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_parse_like_parse_args() {
        assert_eq!(
            parse(&strings(&[
                "--only",
                "claude,codex",
                "--dry-run",
                "--profile",
                "hub"
            ])),
            Parsed::Run(Args {
                dry_run: true,
                only: "claude,codex".into(),
                profile: Some("hub".into()),
                ..Args::default()
            })
        );
        assert_eq!(
            parse(&strings(&["--skip", "--force"])),
            Parsed::Invalid("Option --skip requires a comma-separated value".into())
        );
        assert_eq!(
            parse(&strings(&["--profile"])),
            Parsed::Invalid("Option --profile requires a profile name".into())
        );
        assert_eq!(parse(&strings(&["--force", "-h", "--bogus"])), Parsed::Help);
        assert_eq!(
            parse(&strings(&["--bogus"])),
            Parsed::Invalid("Unknown option: --bogus".into())
        );
    }

    #[test]
    fn a_selection_matches_whole_slugs_from_either_list() {
        let selection = Selection {
            only: "claude,codex".into(),
            skip: "codex".into(),
            profile: None,
        };
        assert!(selection.includes("claude"));
        assert!(!selection.includes("codex"));
        assert!(!selection.includes("claud"));
        assert!(Selection::default().includes("anything"));
    }
}
```

- [ ] **Step 5: Add the command to `src/cli/mod.rs`**

```rust
pub mod check;
pub mod list;
pub mod sync;

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
    /// Distribute `.ai/src` to every enabled tool. `lib/sync.sh` parses its
    /// own options, messages and usage included, so they pass through as text.
    #[command(disable_help_flag = true)]
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
```

- [ ] **Step 6: Replace `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::log::{Sink, Stream};
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
                skip_post_sync: Some("true".to_string()),
                allow_post_sync: None,
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
        Command::Sync { args } => {
            let root = project_root()?;
            Ok(cli::sync::run(
                &root,
                &args,
                &sync_env(),
                log_colors(),
                streams(),
            ))
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn sync_env() -> cli::sync::Env {
    cli::sync::Env {
        render: Env {
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            skip_post_sync: var("AGENTSYNC_SKIP_POST_SYNC"),
            allow_post_sync: var("AGENTSYNC_ALLOW_POST_SYNC"),
        },
        skip_backup: var("AGENTSYNC_INTERNAL_SKIP_BACKUP").as_deref() == Some("true"),
        backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
        backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
    }
}

/// `_use_colors` of `logging.sh`: stdout is a terminal and `NO_COLOR` is empty.
fn log_colors() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && var("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Log lines to the process streams as `echo` writes them. A closed stdout does
/// not stop the run: the transaction finishes, as it would with nobody reading.
fn streams() -> Sink {
    Box::new(|stream, line| {
        let _ = match stream {
            Stream::Out => writeln!(std::io::stdout(), "{line}"),
            Stream::Err => writeln!(std::io::stderr(), "{line}"),
        };
    })
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

- [ ] **Step 7: Append the integration tests to `tests/cli.rs`**

```rust

#[cfg(unix)]
fn sync_project(tool_yaml: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ai/src/rules")).unwrap();
    std::fs::write(dir.path().join(".ai/src/AGENTS.md"), "# Agents\n").unwrap();
    std::fs::write(dir.path().join(".ai/src/rules/core.md"), "# Core\n").unwrap();
    std::fs::write(
        dir.path().join(".ai/agent_sync.yaml"),
        "outputs: committed\ntools:\n  enabled: [claude]\n",
    )
    .unwrap();
    if let Some(yaml) = tool_yaml {
        std::fs::create_dir_all(dir.path().join(".ai/src/tools")).unwrap();
        std::fs::write(dir.path().join(".ai/src/tools/claude.yaml"), yaml).unwrap();
    }
    dir
}

#[cfg(unix)]
fn sync_in(dir: &tempfile::TempDir) -> Command {
    let mut command = agentsync();
    command
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .env_remove("AGENTSYNC_ALLOW_POST_SYNC")
        .env_remove("AGENTSYNC_SKIP_POST_SYNC")
        .arg("sync");
    command
}

#[cfg(unix)]
#[test]
fn sync_options_are_checked_before_anything_runs() {
    let dir = sync_project(None);
    sync_in(&dir)
        .arg("--bogus")
        .assert()
        .code(1)
        .stderr("[ERROR] Unknown option: --bogus\n")
        .stdout(predicate::str::starts_with(
            "AgentSync Config Sync Script\n\nUsage: sync.sh [OPTIONS]\n",
        ));
    sync_in(&dir)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::ends_with(
            "  --help            Show this help message\n",
        ));
    assert!(!dir.path().join("CLAUDE.md").exists());
}

#[cfg(unix)]
#[test]
fn sync_writes_outputs_then_refuses_to_overwrite_a_manual_edit_unless_forced() {
    let dir = sync_project(None);
    sync_in(&dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("[INFO] Syncing Claude Code...\n"))
        .stdout(predicate::str::contains(
            "[DONE] Synced 1/13 tools (12 skipped)\n",
        ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "# Agents\n"
    );
    assert!(dir.path().join(".ai/.sync-manifest").is_file());
    assert!(dir.path().join(".ai/backups/.latest").is_file());

    std::fs::write(dir.path().join("CLAUDE.md"), "edited\n").unwrap();
    sync_in(&dir).assert().code(1).stderr(predicate::str::starts_with(
        "[ERROR] Manual edits detected in 1 destination file(s) since last sync:\n      CLAUDE.md\n",
    ));
    sync_in(&dir)
        .arg("--force")
        .assert()
        .success()
        .stderr(predicate::str::contains("      CLAUDE.md\n"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "# Agents\n"
    );
}

#[cfg(unix)]
#[test]
fn a_failing_post_sync_hook_restores_the_pre_sync_state() {
    let dir = sync_project(Some("post_sync: \"false\"\n"));
    std::fs::write(dir.path().join("CLAUDE.md"), "before-sync\n").unwrap();
    sync_in(&dir)
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "[WARNING] Sync failed; restoring pre-sync state...\n[INFO] Restored pre-sync state from ",
        ))
        .stderr(predicate::str::contains(
            "[ERROR] Sync failed because post-sync hook failed for Claude Code\n",
        ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "before-sync\n"
    );
    assert!(!dir.path().join(".claude/rules").exists());
    assert!(!dir.path().join(".ai/.sync-manifest").exists());
}
```

- [ ] **Step 8: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build --release
bats --jobs 4 tests/check.bats tests/base_skills.bats tests/profiles.bats tests/sync_options.bats --tap | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/check.bats tests/native_parity.bats --tap | grep -c '^not ok'
```

Expected: `142 passed` (unit) and `10 passed` (integration); fmt and clippy exit 0; `0` and `0` — `check` still renders through the stages, forced and in memory, and its 24 parity fixtures agree.

- [ ] **Step 9: Compare a direct native sync with `lib/sync.sh`**

```bash
bash -c '
set -u
ENGINE=$PWD; BIN=$ENGINE/target/release/agentsync
export AGENTSYNC_HOME=$ENGINE AGENTSYNC_NATIVE=0 AGENTSYNC_NO_UPDATE_CHECK=1
base=$(mktemp -d "${TMPDIR:-/tmp}/sync-cmp.XXXXXX")
mkdir "$base/seed" && cd "$base/seed" && git init -q
bash "$ENGINE/bin/agentsync.sh" init --no-detect --no-sync >/dev/null 2>&1
bash "$ENGINE/bin/agentsync.sh" enable claude codex cursor --no-scaffold >/dev/null
printf "# Hand-written\n" > AGENTS.md
cp -pR "$base/seed" "$base/bash"; cp -pR "$base/seed" "$base/native"
mask() { sed -E -e "s#$base/(bash|native)#<root>#g" -e "s#[^ ]*/agentsync_shared\.[A-Za-z0-9]+/src#<overlay>/src#g" \
    -e "s#/<agentsync-overlay>/[a-z-]+/src#<overlay>/src#g" -e "s#[^ ]*/lib/templates/#<engine>/#g" -e "s#[0-9]{8}T[0-9]{6}Z-sync-[0-9]+#<id>#g"; }
(cd "$base/bash" && AGENTSYNC_REPO_ROOT="$base/bash" bash "$ENGINE/lib/sync.sh" 2>&1; echo "status $?") | mask > "$base/bash.out"
(cd "$base/native" && AGENTSYNC_REPO_ROOT="$base/native" "$BIN" sync 2>&1; echo "status $?") | mask > "$base/native.out"
diff "$base/bash.out" "$base/native.out" && echo "same output"
diff -r -x .git -x backups "$base/bash" "$base/native" && echo "same tree"
rm -rf "$base"'
```

Expected: `same output` and `same tree`. Task 10's fixtures make this comparison permanent.

- [ ] **Step 10: Commit**

```bash
git add src/render.rs src/overlay.rs src/paths.rs src/cli/sync.rs src/cli/mod.rs src/main.rs tests/cli.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): run sync with its manifest, backup, and .gitignore transaction"
```

---

### Task 7: `sync` and `--workspace` Served Natively

**Files:**
- Modify: `src/paths.rs` (`find_workspace_ai_dirs`, one test)
- Create: `src/cli/workspace.rs`
- Modify: `src/cli/mod.rs` (whole file)
- Modify: `src/main.rs` (whole file)
- Modify: `bin/agentsync.sh:280` (`_NATIVE_COMMANDS`)
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` ("Known quirks", "Accepted deviations")

**Interfaces:**
- Consumes: `cli::sync::{run, Env}` (Task 6), `style::Style`.
- Produces: `paths::find_workspace_ai_dirs(root: &str) -> Vec<String>` (deepest first, then byte order; `.git`, `node_modules`, a found `.ai`, and symlinks are not entered); `cli::workspace::run(cwd: &str, args: &[String], env: &sync::Env, style: &Style, colors: bool, streams: &dyn Fn() -> Sink) -> u8` (`cmd_workspace_fanout`); `main` routes `sync` with `--workspace` anywhere in its arguments to the fan-out with the flag removed. From this task `agentsync sync` with a built binary is answered natively.

- [ ] **Step 1: `find_workspace_ai_dirs` in `src/paths.rs`**

Add after `ai_dir_enclosing_root`:

```rust
/// `find_workspace_ai_dirs`: every `.ai` directory below `root` holding `src/`
/// or `agent_sync.yaml`, deepest first and then in byte order. `.git` and
/// `node_modules` are not entered, nor is a `.ai` once found, nor a symlink.
pub fn find_workspace_ai_dirs(root: &str) -> Vec<String> {
    fn walk(dir: &str, found: &mut Vec<String>) {
        let name = leaf(dir);
        if name == ".git" || name == "node_modules" {
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(dir) else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        if name == ".ai" {
            found.push(dir.to_string());
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let child = entry.file_name().to_string_lossy().into_owned();
            walk(&format!("{}/{child}", dir.trim_end_matches('/')), found);
        }
    }
    if !Path::new(root).is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    walk(root, &mut found);
    found.retain(|ai| {
        Path::new(&format!("{ai}/src")).is_dir()
            || Path::new(&format!("{ai}/agent_sync.yaml")).is_file()
    });
    found.sort_by(|a, b| {
        let depth = |p: &str| p.split('/').count();
        depth(b).cmp(&depth(a)).then_with(|| a.cmp(b))
    });
    found
}
```

Add this test in the `tests` module, before `fn a_directory_inside_an_ai_tree_names_the_project_above_its_shallowest_ai`:

```rust
    #[cfg(unix)]
    #[test]
    fn workspace_projects_are_listed_deepest_first_and_skip_vendored_trees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        for rel in [
            ".ai/src",
            "b/.ai/src",
            "a/.ai",
            "a/deep/.ai/src/.ai/src",
            "node_modules/pkg/.ai/src",
            ".git/odd/.ai/src",
            ".ai/backups/x/files/.ai/src",
            "bare/.ai",
        ] {
            std::fs::create_dir_all(dir.path().join(rel)).unwrap();
        }
        std::fs::write(dir.path().join("a/.ai/agent_sync.yaml"), "").unwrap();
        std::os::unix::fs::symlink(dir.path().join("b"), dir.path().join("link")).unwrap();
        assert_eq!(
            find_workspace_ai_dirs(&root),
            [
                format!("{root}/a/deep/.ai"),
                format!("{root}/a/.ai"),
                format!("{root}/b/.ai"),
                format!("{root}/.ai"),
            ]
        );
    }
```

- [ ] **Step 2: Create `src/cli/workspace.rs`**

```rust
//! `agentsync sync --workspace`: `cmd_workspace_fanout` of `bin/agentsync.sh`,
//! one sync per project below the working directory, deepest first.

use crate::cli::sync::{self, Env};
use crate::log::{Sink, Stream};
use crate::paths;
use crate::style::Style;

/// Syncs every project below `cwd` with `args`, `--workspace` removed. The
/// status is that of the last project that failed, or 0.
pub fn run(
    cwd: &str,
    args: &[String],
    env: &Env,
    style: &Style,
    colors: bool,
    streams: &dyn Fn() -> Sink,
) -> u8 {
    let mut emit = streams();
    let projects = paths::find_workspace_ai_dirs(cwd);
    if projects.is_empty() {
        emit(
            Stream::Err,
            &format!(
                "{}: No .ai/ directories found below {cwd}",
                style.red("Error")
            ),
        );
        emit(
            Stream::Err,
            &format!(
                "Run {} to create one, or run from a workspace root.",
                style.cyan("agentsync init")
            ),
        );
        return 1;
    }

    emit(Stream::Out, "");
    emit(Stream::Out, &style.bold("  AgentSync workspace sync"));
    emit(
        Stream::Out,
        &style.dim(&format!(
            "  Found {} project(s) below {cwd}",
            projects.len()
        )),
    );
    emit(Stream::Out, "");

    let mut last_failure = 0;
    for ai in &projects {
        let root = paths::parent(ai);
        let rel = if root == cwd {
            ".".to_string()
        } else {
            root.strip_prefix(&format!("{cwd}/"))
                .unwrap_or(&root)
                .to_string()
        };
        emit(Stream::Out, &format!("  {} {rel}", style.cyan("→")));
        let status = sync::run(&root, args, env, colors, streams());
        if status != 0 {
            last_failure = status;
        }
        emit(Stream::Out, "");
    }

    if last_failure == 0 {
        emit(
            Stream::Out,
            &format!(
                "  {} {} project(s) processed.",
                style.green("Workspace sync complete."),
                projects.len()
            ),
        );
    } else {
        emit(
            Stream::Out,
            &format!(
                "  {} max exit code: {last_failure}",
                style.yellow("Workspace sync finished with errors.")
            ),
        );
    }
    emit(Stream::Out, "");
    last_failure
}
```

- [ ] **Step 3: Declare it in `src/cli/mod.rs`**

```rust
pub mod check;
pub mod list;
pub mod sync;
pub mod workspace;

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
    /// Distribute `.ai/src` to every enabled tool. `lib/sync.sh` parses its
    /// own options, messages and usage included, so they pass through as text.
    #[command(disable_help_flag = true)]
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
```

- [ ] **Step 4: Route the fan-out in `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::log::{Sink, Stream};
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
                skip_post_sync: Some("true".to_string()),
                allow_post_sync: None,
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
        Command::Sync { args } if args.iter().any(|a| a == "--workspace") => {
            let forwarded: Vec<String> = args.into_iter().filter(|a| a != "--workspace").collect();
            let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
            let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
            Ok(cli::workspace::run(
                &cwd,
                &forwarded,
                &sync_env(),
                &Style::for_stdout(),
                log_colors(),
                &streams,
            ))
        }
        Command::Sync { args } => {
            let root = project_root()?;
            Ok(cli::sync::run(
                &root,
                &args,
                &sync_env(),
                log_colors(),
                streams(),
            ))
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn sync_env() -> cli::sync::Env {
    cli::sync::Env {
        render: Env {
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            skip_post_sync: var("AGENTSYNC_SKIP_POST_SYNC"),
            allow_post_sync: var("AGENTSYNC_ALLOW_POST_SYNC"),
        },
        skip_backup: var("AGENTSYNC_INTERNAL_SKIP_BACKUP").as_deref() == Some("true"),
        backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
        backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
    }
}

/// `_use_colors` of `logging.sh`: stdout is a terminal and `NO_COLOR` is empty.
fn log_colors() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && var("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Log lines to the process streams as `echo` writes them. A closed stdout does
/// not stop the run: the transaction finishes, as it would with nobody reading.
fn streams() -> Sink {
    Box::new(|stream, line| {
        let _ = match stream {
            Stream::Out => writeln!(std::io::stdout(), "{line}"),
            Stream::Err => writeln!(std::io::stderr(), "{line}"),
        };
    })
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

- [ ] **Step 5: Delegate `sync` in `bin/agentsync.sh`**

Replace:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check "
```

with:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync "
```

- [ ] **Step 6: Record the quirk and the deviations in the design spec**

Append to "Known quirks to reproduce now and fix after cutover", after item 11:

```markdown
12. `sync --workspace` reports the status of the last project that failed as
    "max exit code" and exits with it (`bin/agentsync.sh`, `cmd_workspace_fanout`).
```

Append to "Accepted deviations", after the last Phase 2 line:

```markdown
- Phase 3: `sync`'s step lines name `/<agentsync>/lib/templates` and
  `/<agentsync-overlay>/<layer>/src` where Bash printed the engine checkout and
  its temporary overlay directories.
- Phase 3: an edited install-directory `lib/config.yaml` does not enable
  post-sync hooks; the binary reads the shipped `post_sync.allow: false`, and
  `AGENTSYNC_ALLOW_POST_SYNC=true` enables them as before. Phase 5 settles the
  user-level setting when the install directory goes away.
- Phase 3: a failed write, copy, or removal reports the Rust I/O error where
  Bash printed the `cp`, `mkdir`, or `rm` message; the status and the restore
  are unchanged.
- Phase 3: `sync` keeps running when nothing reads its stdout and finishes its
  transaction; Bash died of `SIGPIPE` at its next log line and left the run
  half written.
- Phase 3: a trapped `INT`, `TERM`, or `HUP` takes effect at the next step of
  `sync` or once `rollback`'s restore returns; Bash's trap fired after the
  running command. A second signal does not interrupt the restore.
- Phase 3: symlinks inside a synced skill, rule, or command tree are copied as
  the files they point to; Bash's `cp -r` copied the links.
- Phase 3: the guard hook's `chmod +x` adds execute permission for every class,
  as the default umask does, whatever the process umask.
- Phase 3: on a terminal a log message prints as written; Bash's `echo -e` also
  expanded backslash escapes inside it.
```

- [ ] **Step 7: Run the gates and the whole suite in both modes, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build --release
AGENTSYNC_NATIVE=1 bats tests/workspace.bats
bats --jobs 4 tests/ --tap | head -1
bats --jobs 4 tests/ --tap | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: `143 passed` (unit) and `10 passed` (integration); fmt and clippy exit 0; `1..7`, 7 ok; `1..766` (762 + Tasks 0b, 0c, 0d); `0` in both modes — every `run_agentsync sync` in the suite, `drift`, `outputs_mode`, `team_workflow`, `workspace`, `version_pin`, `baseline`, `shared`, `profiles`, `opencode`, and the rest now reach the binary; ShellCheck exits 0.

- [ ] **Step 8: Commit**

```bash
git add src/paths.rs src/cli/workspace.rs src/cli/mod.rs src/main.rs bin/agentsync.sh docs/specs/2026-09-12-rust-migration-design.md docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): serve sync and its workspace fan-out natively"
```

---

### Task 8: Signal-Safe Restore

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock` (`signal-hook`, decision 2)
- Create: `src/interrupt.rs`
- Modify: `src/lib.rs`, `src/session.rs` (whole files)
- Modify: `src/render.rs` (`checkpoint`; `run_passes`, `sync_tool`, `run_post_sync_hook`)
- Modify: `src/cli/sync.rs` (arm, checkpoints, re-raise)
- Modify: `tests/cli.rs` (one integration test)

**Interfaces:**
- Consumes: `cli::sync` (Task 6), `Session` (Task 2).
- Produces: `interrupt::Interrupt::arm() -> Interrupt` records `INT`, `TERM`, and (on Unix) `HUP` until dropped; `Interrupt::received(&self) -> Option<i32>`; `Interrupt::resend(&mut self, sig: i32)` restores the default action and raises the signal again, as `kill -$sig $$` does; `interrupt::status(sig: i32) -> u8` is `128 + sig`. `Session::interrupt: Option<Interrupt>` and `Session::interrupted(&self) -> Option<u8>`. `render::checkpoint(s: &Session) -> Step` stops the run with `128 + sig`. `cli::sync` arms the trap right before the backup, checks at every tool, every step of a tool, after a post-sync hook, and around the `.gitignore` update, restores through the failure path, and dies of the signal.

- [ ] **Step 1: Add the signal crate**

Run: `cargo add signal-hook@0.4 --no-default-features`
Expected: `Cargo.toml` gains `signal-hook = { version = "0.4", default-features = false }` after `sha2`.

- [ ] **Step 2: Create `src/interrupt.rs`**

```rust
//! The `INT`, `TERM`, and `HUP` traps a transaction arms in `lib/sync.sh` and
//! `rollback`: a signal is recorded instead of killing the process, the run
//! stops at its next step and restores, and then dies of the same signal.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use signal_hook::SigId;
use signal_hook::consts::signal;

#[cfg(unix)]
const SIGNALS: [i32; 3] = [signal::SIGINT, signal::SIGTERM, signal::SIGHUP];
#[cfg(windows)]
const SIGNALS: [i32; 2] = [signal::SIGINT, signal::SIGTERM];

/// Trapped signals until dropped.
pub struct Interrupt {
    received: Arc<AtomicUsize>,
    ids: Vec<SigId>,
}

impl Interrupt {
    /// Records `INT`, `TERM`, and `HUP` from here on. A signal that cannot be
    /// trapped keeps its default action.
    pub fn arm() -> Self {
        let received = Arc::new(AtomicUsize::new(0));
        let ids = SIGNALS
            .iter()
            .filter_map(|&sig| {
                signal_hook::flag::register_usize(sig, Arc::clone(&received), sig as usize).ok()
            })
            .collect();
        Self { received, ids }
    }

    /// The signal received since `arm`, if any.
    pub fn received(&self) -> Option<i32> {
        match self.received.load(Ordering::SeqCst) {
            0 => None,
            sig => i32::try_from(sig).ok(),
        }
    }

    /// `kill -$sig $$` after the trap: the default action runs, so the parent
    /// sees the signal, not an exit status. Returns only if it did not.
    pub fn resend(&mut self, sig: i32) {
        self.disarm();
        let _ = signal_hook::low_level::emulate_default_handler(sig);
    }

    fn disarm(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// `128 + n`, the status Bash reports for a death by signal `n`.
pub fn status(sig: i32) -> u8 {
    u8::try_from(128 + sig).unwrap_or(u8::MAX)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_trapped_signal_is_recorded_instead_of_ending_the_process() {
        let interrupt = Interrupt::arm();
        assert_eq!(interrupt.received(), None);
        signal_hook::low_level::raise(signal::SIGHUP).unwrap();
        assert_eq!(interrupt.received(), Some(signal::SIGHUP));
        assert_eq!(status(signal::SIGHUP), 129);
        assert_eq!(status(signal::SIGINT), 130);
    }
}
```

- [ ] **Step 3: Declare the module in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod backup;
pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod file_ops;
pub mod filters;
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
pub mod render;
pub mod rules;
pub mod session;
pub mod staging;
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

- [ ] **Step 4: Replace `src/session.rs`**

```rust
//! State one render shares across its steps: the workspace, path rules, the
//! log, the run's `--dry-run` and `--force`, and the manifest's record of what
//! this run wrote (`manifest.sh`).

use std::collections::BTreeSet;

use crate::interrupt::Interrupt;
use crate::log::Log;
use crate::paths::Paths;
use crate::workspace::Workspace;

pub struct Session {
    pub ws: Workspace,
    pub paths: Paths,
    pub log: Log,
    pub dry_run: bool,
    pub force: bool,
    pub interrupt: Option<Interrupt>,
    manifest: Option<BTreeSet<String>>,
    preserved: usize,
    touched: BTreeSet<String>,
    legacy_payload_warned: bool,
}

impl Session {
    pub fn new(ws: Workspace, paths: Paths) -> Self {
        Self {
            ws,
            paths,
            log: Log::default(),
            dry_run: false,
            force: false,
            interrupt: None,
            manifest: None,
            preserved: 0,
            touched: BTreeSet::new(),
            legacy_payload_warned: false,
        }
    }

    pub fn display(&self, path: &str) -> String {
        self.paths.display(path)
    }

    /// The exit status of a trapped signal that arrived, once one has.
    pub fn interrupted(&self) -> Option<u8> {
        self.interrupt
            .as_ref()?
            .received()
            .map(crate::interrupt::status)
    }

    /// `SYNC_MANIFEST_ACTIVE="true"` with `MANIFEST_KEYS` loaded: from here on
    /// a sweep keeps what the previous sync did not generate.
    pub fn activate_manifest(&mut self, paths: BTreeSet<String>) {
        self.manifest = Some(paths);
    }

    /// `sync_may_prune`: outside a manifest-aware run, or under `--force`,
    /// every extraneous entry may go; otherwise a file the manifest records, or
    /// a directory it records a file below.
    pub fn may_prune(&self, abs: &str) -> bool {
        let Some(manifest) = &self.manifest else {
            return true;
        };
        if self.force {
            return true;
        }
        self.paths.to_repo_relative(abs).is_none_or(|rel| {
            let below = format!("{rel}/");
            manifest.contains(&rel)
                || (self.ws.is_dir(abs)
                    && manifest
                        .range(below.clone()..)
                        .next()
                        .is_some_and(|key| key.starts_with(&below)))
        })
    }

    /// `sync_note_preserved`.
    pub fn note_preserved(&mut self, shown: &str) {
        if self.dry_run {
            self.log.warning(&format!(
                "Would keep {shown} (not from .ai/src/; --force to prune)"
            ));
        } else {
            self.log.warning(&format!(
                "Kept {shown} (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)"
            ));
        }
        self.preserved += 1;
    }

    pub fn preserved(&self) -> usize {
        self.preserved
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

    #[test]
    fn only_manifest_paths_may_be_pruned_once_the_manifest_is_active_unless_forced() {
        let mut s = test_session();
        s.ws.create_dir_all("/proj/.claude/skills/old").unwrap();
        s.ws.create_dir_all("/proj/.claude/skills/mine").unwrap();
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
        s.activate_manifest(BTreeSet::from([
            ".claude/rules/old.md".to_string(),
            ".claude/skills/old/SKILL.md".to_string(),
        ]));
        assert!(s.may_prune("/proj/.claude/rules/old.md"));
        assert!(!s.may_prune("/proj/.claude/rules/mine.md"));
        assert!(s.may_prune("/proj/.claude/skills/old"));
        assert!(!s.may_prune("/proj/.claude/skills/mine"));
        assert!(!s.may_prune("/proj/.claude/skills/ol"));
        assert!(s.may_prune("/elsewhere/mine.md"));
        s.force = true;
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
    }

    #[test]
    fn a_preserved_entry_is_counted_and_worded_for_the_run() {
        let mut s = test_session();
        s.note_preserved(".claude/rules/mine.md");
        s.dry_run = true;
        s.note_preserved(".claude/rules/other.md");
        assert_eq!(s.preserved(), 2);
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] Kept .claude/rules/mine.md (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "[WARNING] Would keep .claude/rules/other.md (not from .ai/src/; --force to prune)"
            ]
        );
    }
}
```

- [ ] **Step 5: Checkpoints in `src/render.rs`**

In `run_passes`, add `checkpoint(s)?;` directly after each of the two `run.total += 1;` lines.

Replace `sync_tool` with:

```rust
/// `sync_tool`.
fn sync_tool(s: &mut Session, run: &mut Run, slug: &str) -> Step {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    if !run.selection.includes(slug) {
        run.skipped_names.push(display);
        run.skipped += 1;
        run.printed = false;
        return Ok(());
    }
    run.printed = true;
    let dests = resolve_dests(s, &tool, &display);
    s.log.info(&format!("Syncing {display}..."));

    if !dests.agents.is_empty() {
        let src = tool_source(s, &tool, "agents", &run.sources.agents, &display)?;
        file_ops::copy_file(s, &src, &dests.agents).map_err(|e| io(s, e))?;
    }
    checkpoint(s)?;
    sync_rules_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_skills_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_commands_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_subagents_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_payloads_step(s, &tool, &dests)?;
    checkpoint(s)?;

    let post_sync = tool.value("post_sync");
    if !s.dry_run && !run_post_sync_hook(s, run, &display, &post_sync)? {
        s.log.error(&format!(
            "Sync failed because post-sync hook failed for {display}"
        ));
        return Err(Stop(1));
    }
    s.log.success(&format!("{display} complete"));
    run.synced += 1;
    Ok(())
}
```

Replace `run_post_sync_hook` and its doc comment with `checkpoint` followed by the new `run_post_sync_hook`:

```rust
/// Where a trapped signal ends the run: Bash's trap fires once the command in
/// progress returns.
pub fn checkpoint(s: &Session) -> Step {
    match s.interrupted() {
        Some(status) => Err(Stop(status)),
        None => Ok(()),
    }
}
```

```rust
/// `run_post_sync_hook`: false when the hook ran and failed.
fn run_post_sync_hook(
    s: &mut Session,
    run: &Run,
    display: &str,
    command: &str,
) -> Result<bool, Stop> {
    if command.is_empty() {
        return Ok(true);
    }
    if run.skip_post_sync {
        s.log.info(&format!(
            "Skipping post-sync hook for {display} (AGENTSYNC_SKIP_POST_SYNC=true)"
        ));
        return Ok(true);
    }
    if !run.allow_post_sync {
        s.log.warning(&format!(
            "Skipping post-sync hook for {display} (set AGENTSYNC_ALLOW_POST_SYNC=true to enable)"
        ));
        return Ok(true);
    }
    s.log.info(&format!("Running post-sync hook: {command}"));
    let succeeded = std::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(&s.paths.root)
        .status()
        .is_ok_and(|status| status.success());
    checkpoint(s)?;
    if !succeeded {
        s.log.warning("Post-sync hook failed");
    }
    Ok(succeeded)
}
```

- [ ] **Step 6: Arm, check, and re-raise in `src/cli/sync.rs`**

Add to the imports, before `use crate::log::{Log, Sink};`:

```rust
use crate::interrupt::{self, Interrupt};
```

In `run`, replace:

```rust
    let mut tx = Transaction::default();
    match sync(&mut s, &args, env, &mut tx) {
        Ok(()) => 0,
        Err(Stop(status)) => {
            tx.fail(&mut s, env);
            status
        }
    }
}
```

with:

```rust
    let mut tx = Transaction::default();
    let status = match sync(&mut s, &args, env, &mut tx) {
        Ok(()) => 0,
        Err(Stop(status)) => {
            tx.fail(&mut s, env);
            status
        }
    };
    if let Some(mut interrupt) = s.interrupt.take()
        && let Some(sig) = interrupt.received()
    {
        interrupt.resend(sig);
        return interrupt::status(sig);
    }
    status
}
```

In `start_transaction`, add before `match backup::create(&root, "sync", &targets) {`:

```rust
    s.interrupt = Some(Interrupt::arm());
```

In `finalize`, add `render::checkpoint(s)?;` directly after `let root = s.paths.root.clone();`, and again directly after the closing brace of the `if !s.dry_run && run.update_gitignore { … }` block.

- [ ] **Step 7: Add the integration test to `tests/cli.rs`, before `fn a_failing_post_sync_hook_restores_the_pre_sync_state`'s `#[cfg(unix)]`**

```rust
#[cfg(unix)]
#[test]
fn a_terminated_sync_restores_the_pre_sync_state_and_dies_of_the_signal() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;

    let dir = sync_project(Some("post_sync: \"sleep 1\"\n"));
    std::fs::write(dir.path().join("CLAUDE.md"), "before-sync\n").unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_agentsync"))
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .env_remove("AGENTSYNC_SKIP_POST_SYNC")
        .arg("sync")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for line in lines.by_ref() {
        if line.unwrap().starts_with("[INFO] Running post-sync hook: ") {
            break;
        }
    }
    std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    assert_eq!(child.wait().unwrap().signal(), Some(15));
    assert_eq!(
        rest[0],
        "[WARNING] Sync failed; restoring pre-sync state..."
    );
    assert!(rest[1].starts_with("[INFO] Restored pre-sync state from "));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "before-sync\n"
    );
    assert!(!dir.path().join(".claude/rules").exists());
}

```

- [ ] **Step 8: Run the gates, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build --release
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/sync_options.bats tests/drift.bats tests/workspace.bats --tap | grep -c '^not ok'
```

Expected: `144 passed` (unit) and `11 passed` (integration) — `a_terminated_sync_restores_the_pre_sync_state_and_dies_of_the_signal` sends `SIGTERM` during a `sleep 1` hook and sees the restore lines, the pre-sync `CLAUDE.md`, and a death by signal 15; fmt and clippy exit 0; `0`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/interrupt.rs src/lib.rs src/session.rs src/render.rs src/cli/sync.rs tests/cli.rs docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): restore the pre-sync state when a signal interrupts sync"
```

---

### Task 9: `rollback` Served Natively

**Files:**
- Create: `src/prompts.rs`
- Create: `src/cli/rollback.rs`
- Modify: `src/lib.rs`, `src/cli/mod.rs`, `src/main.rs` (whole files)
- Modify: `bin/agentsync.sh:280` (`_NATIVE_COMMANDS`)

**Interfaces:**
- Consumes: `backup` (Task 5), `interrupt` (Task 8).
- Produces: `prompts::is_tty() -> bool` and `prompts::confirm(question: &str, default_yes: bool) -> bool` (question on stderr, answer from `/dev/tty` or `CONIN$`, the default off a terminal); `cli::rollback::USAGE`; `cli::rollback::Env { backup_limit, backup_max_age }`; `cli::rollback::run(supplied_root: &str, args: &[String], env: &Env, confirm: &mut dyn FnMut(&str) -> bool, out: &mut dyn Write, err: &mut dyn Write) -> u8` (`cmd_rollback` with its safety snapshot and recovery); `Command::Rollback { args: Vec<String> }`. `supplied_root` is `AGENTSYNC_REPO_ROOT`, else the logical working directory, as `${AGENTSYNC_REPO_ROOT:-$(pwd)}` reads.

- [ ] **Step 1: Create `src/prompts.rs`**

```rust
//! `lib/helpers/prompts.sh`: questions go to stderr and answers come from the
//! terminal device, so captured output never swallows a prompt.

use std::io::{BufRead, BufReader, IsTerminal, Write};

/// `is_tty`: stdin and stdout are both terminals.
pub fn is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// `prompt_confirm`: off a terminal the default answers.
pub fn confirm(question: &str, default_yes: bool) -> bool {
    if !is_tty() {
        return default_yes;
    }
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    let mut stderr = std::io::stderr();
    let _ = write!(stderr, "{question} {hint} ");
    let _ = stderr.flush();
    let reply = read_terminal_line().unwrap_or_default();
    answer_is_yes(&reply, default_yes)
}

fn answer_is_yes(reply: &str, default_yes: bool) -> bool {
    let reply = reply.trim_matches([' ', '\t', '\n']).to_lowercase();
    match reply.as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    }
}

fn read_terminal_line() -> Option<String> {
    #[cfg(windows)]
    const TERMINAL: &str = "CONIN$";
    #[cfg(not(windows))]
    const TERMINAL: &str = "/dev/tty";
    let tty = std::fs::File::open(TERMINAL).ok()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_read_like_prompt_confirm() {
        assert!(answer_is_yes("Y\n", false));
        assert!(answer_is_yes("  y \n", false));
        assert!(answer_is_yes("yes\n", false));
        assert!(!answer_is_yes("yep\n", true));
        assert!(answer_is_yes("\n", true));
        assert!(!answer_is_yes("", false));
    }
}
```

- [ ] **Step 2: Create `src/cli/rollback.rs`**

```rust
//! `agentsync rollback`: `cmd_rollback` of `lib/helpers/backup.sh`. A safety
//! snapshot is taken first, and a restore that fails or is interrupted puts
//! it back.

use std::io::Write;

use crate::interrupt::{self, Interrupt};
use crate::{Error, backup, paths};

pub const USAGE: &str = "Usage: agentsync rollback [<backup-id>] [OPTIONS]

Restore AgentSync-managed targets from a backup. Without an ID, restores the
latest complete snapshot. A safety snapshot is created before every restore,
so the rollback itself can be undone.

Options:
  --list       List complete backups
  --dry-run    Show the restore plan without changing files
  -y, --yes    Skip the confirmation prompt
  -h, --help   Show this help
";

/// The environment `backup_prune` reads.
#[derive(Default)]
pub struct Env {
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}

/// Runs `rollback` for the project at `supplied_root`; `confirm` answers the
/// restore question when `--yes` is absent. Returns the exit status.
pub fn run(
    supplied_root: &str,
    args: &[String],
    env: &Env,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut backup_id: Option<String> = None;
    let (mut list_only, mut dry_run, mut assume_yes) = (false, false, false);
    for arg in args {
        match arg.as_str() {
            "--list" => list_only = true,
            "--dry-run" => dry_run = true,
            "--yes" | "-y" => assume_yes = true,
            "--help" | "-h" => {
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            option if option.starts_with('-') => {
                let _ = writeln!(err, "Error: Unknown rollback option: {option}");
                let _ = err.write_all(USAGE.as_bytes());
                return 1;
            }
            id if backup_id.is_some() => {
                let _ = writeln!(err, "Error: Unexpected rollback argument: {id}");
                return 1;
            }
            id => backup_id = Some(id.to_string()),
        }
    }

    let fail = |err: &mut dyn Write, e: Error| {
        let _ = match e {
            Error::Backup(message) => writeln!(err, "Error: {message}"),
            other => writeln!(err, "{other}"),
        };
        1
    };
    let root = match backup::canonical_root(supplied_root) {
        Ok(root) => root,
        Err(e) => return fail(err, e),
    };

    if list_only {
        if backup_id.is_some() {
            let _ = writeln!(err, "Error: A backup ID cannot be combined with --list");
            return 1;
        }
        let rows = match backup::list(&root) {
            Ok(rows) => rows,
            Err(e) => return fail(err, e),
        };
        if rows.is_empty() {
            let _ = writeln!(out, "No AgentSync backups found.");
            return 0;
        }
        let _ = writeln!(out, "Backup ID\tOperation\tCreated (UTC)");
        for (id, operation, created) in rows {
            let _ = writeln!(out, "{id}\t{operation}\t{created}");
        }
        return 0;
    }

    let snapshot = match &backup_id {
        Some(id) if *id != paths::leaf(id) => {
            let _ = writeln!(err, "Error: Invalid backup ID: {id}");
            return 1;
        }
        Some(id) => match backup::snapshot_path(&root, id) {
            Ok(snapshot) => snapshot,
            Err(e) => return fail(err, e),
        },
        None => match backup::latest(&root) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                let _ = writeln!(err, "Error: No complete AgentSync backup found");
                return 1;
            }
            Err(e) => return fail(err, e),
        },
    };
    let id = paths::leaf(&snapshot);
    let targets = match backup::load_targets(&root, &snapshot) {
        Ok(targets) => targets,
        Err(e) => return fail(err, e),
    };

    let _ = writeln!(out, "Rollback plan:");
    let _ = writeln!(out, "  Backup: {id}");
    for target in &targets {
        let action = if target.present { "restore" } else { "remove" };
        let _ = writeln!(out, "  {action:<7} {}", target.rel);
    }
    if dry_run {
        let _ = writeln!(out, "Dry run — nothing was written.");
        return 0;
    }
    if !assume_yes && !confirm(&format!("Restore backup {id}?")) {
        let _ = writeln!(out, "Cancelled.");
        return 130;
    }

    let current: Vec<String> = targets
        .iter()
        .map(|target| format!("{root}/{}", target.rel))
        .collect();
    let safety = match backup::create(&root, "rollback", &current) {
        Ok(safety) => safety,
        Err(e) => {
            fail(err, e);
            let _ = writeln!(
                err,
                "Error: Could not create a pre-rollback safety backup; no files were changed"
            );
            return 1;
        }
    };

    let mut interrupt = Interrupt::arm();
    let restored = backup::restore(&root, &snapshot);
    let signal = interrupt.received();
    if restored.is_err() || signal.is_some() {
        let status = match (restored, signal) {
            (_, Some(sig)) => interrupt::status(sig),
            (Err(e), None) => fail(err, e),
            (Ok(()), None) => 0,
        };
        let shown = safety.strip_prefix(&format!("{root}/")).unwrap_or(&safety);
        let _ = writeln!(
            err,
            "Warning: Rollback failed; restoring the state from before rollback..."
        );
        match backup::restore(&root, &safety) {
            Ok(()) => {
                let _ = writeln!(err, "Restored pre-rollback state from {shown}");
            }
            Err(e) => {
                fail(err, e);
                let _ = writeln!(
                    err,
                    "Error: Recovery failed. Safety backup retained at {shown}"
                );
            }
        }
        if let Some(sig) = signal {
            interrupt.resend(sig);
        }
        return status;
    }
    drop(interrupt);

    if let Err(e) = backup::prune(
        &root,
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
    ) {
        fail(err, e);
        let _ = writeln!(err, "Warning: Could not prune old AgentSync backups.");
    }
    let _ = writeln!(out, "Restored backup {id}.");
    let _ = writeln!(out, "Undo backup: {}", paths::leaf(&safety));
    0
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Output {
        status: u8,
        out: String,
        err: String,
    }

    fn rollback(root: &str, args: &[&str], answer: bool) -> Output {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(
            root,
            &args,
            &Env::default(),
            &mut |_| answer,
            &mut out,
            &mut err,
        );
        Output {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::write(dir.path().join("CLAUDE.md"), "before\n").unwrap();
        (dir, root)
    }

    #[test]
    fn a_rollback_restores_the_latest_backup_and_leaves_an_undo_backup() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md"), format!("{root}/.claude/rules")];
        let snapshot = backup::create(&root, "sync", &targets).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "after\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/rules")).unwrap();

        let plan = rollback(&root, &["--dry-run"], false);
        assert_eq!(
            plan.out,
            format!(
                "Rollback plan:\n  Backup: {id}\n  restore CLAUDE.md\n  remove  .claude/rules\nDry run — nothing was written.\n"
            )
        );
        let cancelled = rollback(&root, &[], false);
        assert_eq!(cancelled.status, 130);
        assert!(cancelled.out.ends_with("Cancelled.\n"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "after\n"
        );

        let done = rollback(&root, &["--yes"], false);
        assert_eq!(done.status, 0);
        assert_eq!(done.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        assert!(!dir.path().join(".claude/rules").exists());
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(done.out.ends_with(&format!(
            "Restored backup {id}.\nUndo backup: {}\n",
            paths::leaf(&undo)
        )));
        assert_eq!(
            std::fs::read_to_string(format!("{undo}/metadata"))
                .unwrap()
                .lines()
                .nth(1),
            Some("operation=rollback")
        );

        let listed = rollback(&root, &["--list"], false);
        assert!(
            listed
                .out
                .starts_with("Backup ID\tOperation\tCreated (UTC)\n")
        );
        assert!(listed.out.contains(&format!("{id}\tsync\t")));
    }

    #[test]
    fn arguments_and_ids_are_checked_before_anything_is_read() {
        let (_dir, root) = project();
        let unknown = rollback(&root, &["--nope"], true);
        assert_eq!(unknown.status, 1);
        assert!(
            unknown
                .err
                .starts_with("Error: Unknown rollback option: --nope\nUsage: agentsync rollback")
        );
        assert_eq!(
            rollback(&root, &["a", "b"], true).err,
            "Error: Unexpected rollback argument: b\n"
        );
        assert_eq!(
            rollback(&root, &["../x", "--yes"], true).err,
            "Error: Invalid backup ID: ../x\n"
        );
        assert_eq!(
            rollback(&root, &["x", "--list"], true).err,
            "Error: A backup ID cannot be combined with --list\n"
        );
        assert_eq!(
            rollback(&root, &["--list"], true).out,
            "No AgentSync backups found.\n"
        );
        assert_eq!(
            rollback(&root, &["--yes"], true).err,
            "Error: No complete AgentSync backup found\n"
        );
        assert_eq!(rollback(&root, &["--help", "--nope"], true).out, USAGE);
    }
}
```

- [ ] **Step 3: Declare the modules in `src/lib.rs`**

```rust
//! AgentSync native engine. `main.rs` is the only place that talks to the
//! process (arguments, exit codes); everything here is callable from tests.

pub mod backup;
pub mod catalog;
pub mod cli;
pub mod convert;
pub mod error;
pub mod file_ops;
pub mod filters;
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
pub mod prompts;
pub mod render;
pub mod rules;
pub mod session;
pub mod staging;
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

- [ ] **Step 4: Add the command to `src/cli/mod.rs`**

```rust
pub mod check;
pub mod list;
pub mod rollback;
pub mod sync;
pub mod workspace;

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
    /// Distribute `.ai/src` to every enabled tool. `lib/sync.sh` parses its
    /// own options, messages and usage included, so they pass through as text.
    #[command(disable_help_flag = true)]
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Restore targets from a backup; parses its own options like `cmd_rollback`.
    #[command(disable_help_flag = true)]
    Rollback {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
```

- [ ] **Step 5: Route it in `src/main.rs`**

```rust
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use agentsync::cli::{self, Cli, Command};
use agentsync::log::{Sink, Stream};
use agentsync::project::Project;
use agentsync::render::Env;
use agentsync::style::Style;
use agentsync::{Error, engine_version, paths, prompts};
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
                skip_post_sync: Some("true".to_string()),
                allow_post_sync: None,
            };
            let mut out = std::io::stdout().lock();
            let mut err = std::io::stderr().lock();
            cli::check::run(&root, &env, &mut out, &mut err)
        }
        Command::Sync { args } if args.iter().any(|a| a == "--workspace") => {
            let forwarded: Vec<String> = args.into_iter().filter(|a| a != "--workspace").collect();
            let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
            let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
            Ok(cli::workspace::run(
                &cwd,
                &forwarded,
                &sync_env(),
                &Style::for_stdout(),
                log_colors(),
                &streams,
            ))
        }
        Command::Rollback { args } => {
            let supplied_root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
                Some(root) => root,
                None => {
                    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                    paths::logical_root(None, &cwd, var("PWD").as_deref())
                }
            };
            let env = cli::rollback::Env {
                backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
                backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            };
            let mut confirm = |question: &str| prompts::confirm(question, false);
            Ok(cli::rollback::run(
                &supplied_root,
                &args,
                &env,
                &mut confirm,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            ))
        }
        Command::Sync { args } => {
            let root = project_root()?;
            Ok(cli::sync::run(
                &root,
                &args,
                &sync_env(),
                log_colors(),
                streams(),
            ))
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn sync_env() -> cli::sync::Env {
    cli::sync::Env {
        render: Env {
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            skip_post_sync: var("AGENTSYNC_SKIP_POST_SYNC"),
            allow_post_sync: var("AGENTSYNC_ALLOW_POST_SYNC"),
        },
        skip_backup: var("AGENTSYNC_INTERNAL_SKIP_BACKUP").as_deref() == Some("true"),
        backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
        backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
    }
}

/// `_use_colors` of `logging.sh`: stdout is a terminal and `NO_COLOR` is empty.
fn log_colors() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && var("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Log lines to the process streams as `echo` writes them. A closed stdout does
/// not stop the run: the transaction finishes, as it would with nobody reading.
fn streams() -> Sink {
    Box::new(|stream, line| {
        let _ = match stream {
            Stream::Out => writeln!(std::io::stdout(), "{line}"),
            Stream::Err => writeln!(std::io::stderr(), "{line}"),
        };
    })
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

- [ ] **Step 6: Delegate `rollback` in `bin/agentsync.sh`**

Replace:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync "
```

with:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback "
```

- [ ] **Step 7: Run the gates and the rollback suites, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build --release
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/rollback.bats tests/baseline.bats tests/cli.bats tests/native_dispatch.bats --tap | head -1
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/rollback.bats tests/baseline.bats tests/cli.bats tests/native_dispatch.bats --tap | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: `147 passed` (unit) and `11 passed` (integration); fmt and clippy exit 0; `1..33` (6 + 11 + 8 + 8) with `0` failures — `rollback requires confirmation outside a TTY unless --yes is passed` exits 130 through `prompts::confirm`'s default; ShellCheck exits 0.

- [ ] **Step 8: Commit**

```bash
git add src/prompts.rs src/cli/rollback.rs src/lib.rs src/cli/mod.rs src/main.rs bin/agentsync.sh docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "feat(native): port rollback"
```

---

### Task 10: Parity Fixtures for `sync` and `rollback`, and CI

**Files:**
- Modify: `tests/native_parity.bats` (append the `sync` and `rollback` section)
- Modify: `.github/workflows/ci.yaml` (`native` job)

**Interfaces:**
- Consumes: the binary, `_run_engine`, `_bash_sync`, `enable_tools`, `create_test_symlink`, `ALL_TOOLS`.
- Produces: `_mask_run_paths <dir>` (stdin → stdout), `_assert_same_trees <left> <right>`, and `[PARITY_CWD=<subdir>] assert_tree_parity <args…>` for later phases; ten fixtures covering the spec's Phase 3 list — a fresh project's manifest, `.gitignore`, and backup; every tool on this repository's own `.ai/src`; `--dry-run`, `--only`, `--skip`, option errors, and `--help`; the drift refusal and `--force`; kept untracked outputs and pruned generated ones (Task 0c); disabled-tool cleanup and the three outputs modes; profiles with `--profile`, `shared:` with an unknown category, and a local version pin; `--if-stale` fresh and stale, hooks skipped, run, and failing; a malformed OpenCode MCP source that restores; the refusal inside `.ai`, a symlinked destination escaping the project, and `--workspace`; `rollback` plans, restores, cancels, and refuses; and a Bash `rollback` of a backup the native `sync` wrote.

- [ ] **Step 1: Append to `tests/native_parity.bats`**

```bash

# ── sync and rollback ────────────────────────────────────────────────────────
# Both commands change the project, so each engine runs in its own copy and
# the copies must end identical. Output differs only where the run names its
# own machinery: the project copy, backup ids, the engine checkout Bash read
# templates from, and the temporary overlay directories.

# Usage: _mask_run_paths <dir> < output
_mask_run_paths() {
    local dir="$1" text from
    local root_mask="<root>" engine_mask="<engine>/lib/templates"
    text=$(cat)
    # Bash 3.2 splits a literal pattern at its first slash, so patterns go
    # through variables.
    from=$(cd -P "$dir" && pwd)
    text=${text//"$from"/$root_mask}
    text=${text//"$dir"/$root_mask}
    for from in "$REPO_ROOT/lib/templates" "~/${REPO_ROOT#"$HOME"/}/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/$engine_mask}
    done
    printf '%s\n' "$text" | sed -E \
        -e 's#[^ ]*/agentsync_shared\.[A-Za-z0-9]+/src#<overlay>/src#g' \
        -e 's#/<agentsync-overlay>/[a-z-]+/src#<overlay>/src#g' \
        -e 's#[0-9]{8}T[0-9]{6}Z-(sync|rollback|init)-[0-9]+(-[0-9]+)?#<backup-id>#g'
}

# Usage: _assert_same_trees <left> <right>
# Every path outside .git and the backup store, and every file's bytes.
_assert_same_trees() {
    local left="$1" right="$2" rel
    (cd "$left" && find . \( -path '*/.git' -o -path '*/.ai/backups' \) -prune -o -print | LC_ALL=C sort) > "$left.tree"
    (cd "$right" && find . \( -path '*/.git' -o -path '*/.ai/backups' \) -prune -o -print | LC_ALL=C sort) > "$right.tree"
    if ! diff "$left.tree" "$right.tree" >&2; then
        return 1
    fi
    while IFS= read -r rel; do
        [[ -f "$left/$rel" ]] || continue
        if ! cmp -s "$left/$rel" "$right/$rel"; then
            echo "content differs: $rel" >&2
            diff "$left/$rel" "$right/$rel" >&2 || true
            return 1
        fi
    done < "$left.tree"
}

# Usage: [PARITY_CWD=<subdir>] assert_tree_parity <agentsync args...>
# Runs the command in a copy of the project per engine, from PARITY_CWD
# inside it, and compares status, masked output, and the resulting trees.
assert_tree_parity() {
    local left="$BATS_TEST_TMPDIR/bash" right="$BATS_TEST_TMPDIR/native"
    rm -rf "$left" "$right"
    cp -pR "$TEST_PROJECT" "$left"
    cp -pR "$TEST_PROJECT" "$right"
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(cd "$left/${PARITY_CWD:-.}" && _run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(cd "$right/${PARITY_CWD:-.}" && _run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        printf '%s\n' "$native_out" >&2
        return 1
    fi
    printf '%s\n' "$bash_out" | _mask_run_paths "$left" > "$left.out"
    printf '%s\n' "$native_out" | _mask_run_paths "$right" > "$right.out"
    if ! diff "$left.out" "$right.out" >&2; then
        return 1
    fi
    _assert_same_trees "$left" "$right"
}

@test "parity: sync writes a fresh project and its manifest, .gitignore, and backup" {
    enable_tools claude codex cursor
    printf '# Hand-written\n' > AGENTS.md
    printf '\noutputs: local\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync of every tool and this repository's own .ai/src" {
    rm -rf .ai/src
    cp -R "$REPO_ROOT/.ai/src" .ai/src
    enable_tools "${ALL_TOOLS[@]}"
    assert_tree_parity sync
}

@test "parity: sync dry-run, --only, --skip, and option errors" {
    enable_tools claude codex gemini
    assert_tree_parity sync --dry-run
    assert_tree_parity sync --only codex,gemini --skip gemini
    assert_tree_parity sync --only
    assert_tree_parity sync --bogus
    assert_tree_parity sync --help
}

@test "parity: sync refuses a manual edit, keeps untracked outputs, and prunes what it generated" {
    enable_tools claude cursor
    mkdir -p .ai/src/skills/temp-skill
    printf -- '---\nname: temp-skill\n---\n' > .ai/src/skills/temp-skill/SKILL.md
    printf '# Temp\n' > .ai/src/rules/temp.md
    _bash_sync
    rm -rf .ai/src/skills/temp-skill .ai/src/rules/temp.md
    printf 'mine\n' > .claude/rules/mine.md
    mkdir -p .claude/skills/mine
    printf 'mine\n' > .claude/skills/mine/SKILL.md
    assert_tree_parity sync --dry-run
    assert_tree_parity sync
    printf 'edited\n' >> CLAUDE.md
    assert_tree_parity sync
    assert_tree_parity sync --force
}

@test "parity: sync cleans a disabled tool and follows the outputs mode" {
    enable_tools claude cursor codex
    _bash_sync
    _run_engine 0 disable cursor >/dev/null
    printf '\noutputs: committed\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    printf '\ngitignore:\n  update: false\n' >> .ai/agent_sync.yaml
    sed 's/^outputs: committed$//' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    assert_tree_parity sync
    printf '\noutputs: shared\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync with profiles, shared inheritance, and a version pin" {
    enable_tools claude
    _run_engine 0 profile add hub --tools claude,codex >/dev/null
    mkdir -p .ai/profiles/hub/src/rules parent/.ai/src/rules parent/.ai/src/skills/parent-skill
    printf '# Hub only\n' > .ai/profiles/hub/src/rules/hub.md
    printf '# Parent\n' > parent/.ai/src/rules/parent.md
    printf 'p\n' > parent/.ai/src/skills/parent-skill/SKILL.md
    printf '\nshared:\n  path: parent\n  inherit: rules, skills, tools\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    assert_tree_parity sync --profile hub --only claude-hub
    printf '\nagentsync_version: "0.0.1"\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync --if-stale, post-sync hooks, and a failed run's restore" {
    enable_tools claude opencode
    _bash_sync
    touch -t 203001010000 .ai/.sync-manifest
    assert_tree_parity sync --if-stale
    touch -t 200001010000 .ai/.sync-manifest
    assert_tree_parity sync --if-stale
    mkdir -p .ai/src/tools
    printf 'post_sync: "printf hooked > hooked.txt"\n' > .ai/src/tools/claude.yaml
    assert_tree_parity sync
    AGENTSYNC_ALLOW_POST_SYNC=true assert_tree_parity sync
    printf 'post_sync: "false"\n' > .ai/src/tools/claude.yaml
    AGENTSYNC_ALLOW_POST_SYNC=true assert_tree_parity sync
    printf '%s\n' '{"mcpServers":[]}' > .ai/src/mcp.json
    rm -f .ai/src/tools/claude.yaml
    assert_tree_parity sync
}

@test "parity: sync from inside .ai, into a symlinked dest, and across a workspace" {
    enable_tools claude
    PARITY_CWD=.ai/src assert_tree_parity sync
    mkdir -p "$BATS_TEST_TMPDIR/outside"
    create_test_symlink "$BATS_TEST_TMPDIR/outside" .claude
    assert_tree_parity sync
    rm -f .claude
    mkdir -p leaf
    (cd leaf && git init --quiet && _run_engine 0 init --no-detect --no-sync >/dev/null 2>&1)
    (cd leaf && _run_engine 0 enable cursor --no-scaffold >/dev/null)
    assert_tree_parity sync --workspace --dry-run
    assert_tree_parity sync --workspace --only cursor
}

@test "parity: rollback plans, restores, and refuses like Bash" {
    enable_tools claude
    printf 'before-sync\n' > CLAUDE.md
    _bash_sync
    assert_tree_parity rollback --dry-run
    assert_tree_parity rollback --list
    assert_tree_parity rollback
    assert_tree_parity rollback --yes
    assert_tree_parity rollback "../$(cat .ai/backups/.latest)" --yes
    assert_tree_parity rollback nope
    assert_tree_parity rollback --bogus
    assert_tree_parity rollback --help
}

@test "parity: a backup the native sync writes is restored by the Bash rollback" {
    enable_tools claude cursor
    printf 'before-sync\n' > CLAUDE.md
    local left="$BATS_TEST_TMPDIR/bash-made" right="$BATS_TEST_TMPDIR/native-made"
    cp -pR "$TEST_PROJECT" "$left"
    cp -pR "$TEST_PROJECT" "$right"
    (cd "$left" && _run_engine 0 sync >/dev/null 2>&1)
    (cd "$right" && _run_engine 1 sync >/dev/null 2>&1)
    [ "$(cat "$right/.ai/backups/$(cat "$right/.ai/backups/.latest")/targets.tsv")" = \
      "$(cat "$left/.ai/backups/$(cat "$left/.ai/backups/.latest")/targets.tsv")" ]
    (cd "$left" && _run_engine 0 rollback --yes >/dev/null 2>&1)
    (cd "$right" && _run_engine 0 rollback --yes >/dev/null 2>&1)
    _assert_same_trees "$left" "$right"
    [ "$(cat "$right/CLAUDE.md")" = "before-sync" ]
}
```

- [ ] **Step 2: Run the parity suite, confirm green**

```bash
cargo build --release
bats --jobs 4 tests/native_parity.bats --tap | grep -c '^ok'
AGENTSYNC_NATIVE_BIN=/nonexistent bats tests/native_parity.bats --tap | grep -c ' # skip'
```

Expected: `34` (24 before + 10); `34` skipped without a binary. A failure prints the diff between the masked outputs or the trees; fix the native side, never the fixture. The fixture on this repository's own `.ai/src` is the slowest.

- [ ] **Step 3: Confirm a fixture sees a difference**

In `src/session.rs`, change `"Kept {shown} (not from .ai/src/;` to `"Kept! {shown} (not from .ai/src/;`, then:

```bash
cargo build --release
bats tests/native_parity.bats -f 'keeps untracked'
git checkout src/session.rs
cargo build --release
```

Expected: `not ok 1 parity: sync refuses a manual edit, keeps untracked outputs, and prunes what it generated`, with a diff whose `>` lines read `[WARNING] Kept! .claude/rules/mine.md …` and `[WARNING] Kept! .claude/skills/mine …`; after the checkout `git diff --stat src/` is empty.

- [ ] **Step 4: Run the whole suite natively in CI**

In `.github/workflows/ci.yaml`, `native` job, replace the `Ported commands through the Bash suite` step with:

```yaml
      # Every command that writes outputs goes through the native sync from
      # Phase 3 on, so the whole suite is the native contract.
      - name: The Bash suite against the native engine
        if: runner.os != 'Windows'
        shell: bash
        env:
          TERM: xterm
          AGENTSYNC_NATIVE: "1"
        run: bats --jobs 4 --tap tests/
```

- [ ] **Step 5: Run the whole suite in both modes**

```bash
bats --jobs 4 tests/ --tap | head -1
bats --jobs 4 tests/ --tap | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --jobs 4 tests/ --tap | grep -c '^not ok'
```

Expected: `1..776` (766 + 10 parity) and `0` failures in both modes.

- [ ] **Step 6: Commit**

```bash
git add tests/native_parity.bats .github/workflows/ci.yaml docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "test(native): diff Bash against native sync and rollback on disk"
```

---

### Task 11: Module Map and Engine Rule

**Files:**
- Modify: `.ai/src/skills/native-port/references/module-map.md`
- Modify: `.ai/src/rules/native-engine.md`
- Modify: `.ai/.sync-manifest` (regenerated)

**Interfaces:**
- Consumes: the modules as they landed.
- Produces: the map and the rule Phase 4's plans are written from.

- [ ] **Step 1: Update the engine rows of `module-map.md`**

Replace these rows in the "Engine modules" block:

```text
lib/helpers/logging.sh           → src/log.rs              plain [INFO]/[SUCCESS]/[WARNING]/[ERROR], step, separator, tail; display_path is in src/paths.rs
lib/helpers/tmp.sh               → tempfile crate          run dir + staging siblings; tmp_sibling stays on one filesystem
(check.sh tar copy)              → src/workspace.rs        in-memory tree: disk index, embedded engine at /<agentsync>, overlays at /<agentsync-overlay>
lib/helpers/paths.sh             → src/paths.rs            normalise, containment, existing-ancestor canonicalise, repo-relative
lib/helpers/manifest.sh          → src/session.rs (record_write, was_touched, record_tree; Phase 2), src/manifest.rs (load, drift, write; Phase 3)
lib/helpers/shared.sh            → src/overlay.rs          base-src and profile overlays, shared parent merge for check
lib/helpers/backup.sh            → src/backup.rs           same on-disk layout; restore on failure via a Drop guard
lib/helpers/prompts.sh           → src/prompts.rs          confirm and multiselect on /dev/tty
lib/sync.sh                      → src/render.rs (forced render, Phase 2) + src/cli/sync.rs (transaction, Phase 3)
lib/helpers/backup.sh (rollback) → src/cli/rollback.rs     Phase 3
```

with:

```text
lib/helpers/logging.sh           → src/log.rs              plain and coloured tags, [DONE], step, separator, tail, streaming sink; display_path is in src/paths.rs
lib/helpers/tmp.sh               → src/staging.rs          tmp_sibling + mv as write_beside; no run directory, overlays are virtual
(check.sh tar copy)              → src/workspace.rs        in memory for check; on disk for sync, with /<agentsync> and /<agentsync-overlay> kept virtual
lib/helpers/paths.sh             → src/paths.rs            normalise, containment (lexical for check, through the disk for sync), repo-relative, ai_dir_enclosing_root, find_workspace_ai_dirs
lib/helpers/manifest.sh          → src/session.rs (record_write, was_touched, record_tree, may_prune), src/manifest.rs (load, drift, write)
lib/helpers/shared.sh            → src/overlay.rs          shared, base-src, and profile overlays; shared parent merge for check
lib/helpers/backup.sh            → src/backup.rs           same on-disk layout; create, restore, latest, list, prune
lib/helpers/prompts.sh           → src/prompts.rs          confirm on /dev/tty (Phase 3); multiselect waits for Phase 4
(trap INT TERM HUP)              → src/interrupt.rs        signal-hook flags; restore at the next step, then re-raise
lib/sync.sh                      → src/render.rs (stages) + src/cli/sync.rs (transaction); Phase 3, ported
bin/agentsync.sh workspace fan-out → src/cli/workspace.rs  Phase 3, ported
lib/helpers/backup.sh (rollback) → src/cli/rollback.rs     Phase 3, ported
```

- [ ] **Step 2: Describe commands that own their streams in `native-engine.md`**

Replace:

```markdown
- `src/cli/<cmd>.rs` owns one command as `render(…) -> Result<String, Error>` plus `run(…, &mut impl Write)`. Core modules never print.
```

with:

```markdown
- `src/cli/<cmd>.rs` owns one command as `render(…) -> Result<String, Error>` plus `run(…, &mut impl Write)`. A command with its own exit status returns it from `run` — `check` through a `Report`, `sync` and `rollback` as a `u8` — and writes only through the writers or the log sink `main` hands it. Core modules never print.
```

- [ ] **Step 3: Regenerate this repository's agent files**

Run: `bash bin/agentsync.sh sync`
Expected: `Synced 2/13 tools (11 skipped)`, answered by the native binary built in Task 10; `git status --short` lists the two `.ai/src` files and `.ai/.sync-manifest` (outputs are gitignored here). An agent sandbox that denies writes under `.claude/` fails this run whole and restores it; run it outside the sandbox.

- [ ] **Step 4: Commit**

```bash
git add .ai/src/skills/native-port/references/module-map.md .ai/src/rules/native-engine.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md
git commit -m "docs(native): map the phase 3 modules"
```

---

## Completion

Before reporting Phase 3 done, append the completion receipt from `verification.md`:

- Each Global Constraint mapped to the file that satisfies it, including the eight deviations and quirk 12 as they read in the spec, and the decisions the review took.
- Fresh output of: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`, ShellCheck over the shell entry points, and `bats --jobs 4 tests/ --tap` under both `AGENTSYNC_NATIVE=0` and `AGENTSYNC_NATIVE=1`.
- Golden outputs on the 13-tool fixture: `bash scripts/perf/make-fixture.sh <dir>` twice, a Bash `sync` in one copy and a native `sync` in the other, then `diff -r -x .git -x backups` of the copies is empty and a native `check` exits 0 in both.
- `sync` timed on the same fixture in both engines with `bash scripts/perf/bench.sh --runs 3`, next to Phase 2's 55.15 s Bash best, and `sync --if-stale` beside it.
- The terminal check: a Bash and a native `sync` under `script(1)` produce the same bytes once run paths are masked.
- The `native` CI job's result on the branch, or the line recorded as open when the branch is still unpushed.
- Anything skipped or deferred.

---

## Run log

### 2026-09-14 — Phase 3 planned
- Commits: `docs(native): plan phase 3`
- Verified: Phase 2's plan has no unchecked box and carries its receipt. Before writing, the whole phase was built in a scratch worktree of `0217ec4`, and its task commits were then replayed in plan order in a second worktree: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass after every task with the counts the steps state (112, 112, 112, 115, 120, 126, 129, 138, 142 + 10, 143 + 10, 144 + 11, 147 + 11). The three Bash fixes were red before and green after (`baseline.bats` 11, `drift.bats` 28, `gitignore.bats` 7), and the files they touch ran with 0 failures. The cross-checks of Tasks 2, 3, 4, and 5 printed the values their tests assert, and the coloured prefixes under `script(1)` are the bytes the streaming log test asserts. At Task 6 the Bash `check`, `base_skills`, `profiles`, and `sync_options` files and the native `check` and 24 parity fixtures ran with 0 failures, and a direct native `sync` matched `lib/sync.sh` in output and tree. At Task 7 `workspace.bats` passed natively and the whole suite ran `1..766` with 0 failures in both modes. At Task 8 the native `sync_options`, `drift`, and `workspace` files had 0 failures and the `SIGTERM` test died of signal 15 after restoring. At Task 9 the four rollback-facing files ran `1..33` with 0 failures. At Task 10 `tests/native_parity.bats` → 34 ok, 34 skipped without a binary, and the whole suite `1..776` with 0 failures in both modes; changing one native message made its fixture fail with the diff. A Bash and a native `sync` under `script(1)` produced the same bytes once run paths were masked. With all of it in place, `bash bin/agentsync.sh sync` on this repository answered natively with `Synced 2/13 tools (11 skipped)`.
- Plan amended: none; this run wrote it.
- Next: the plan's review. Four decisions ride on it: Task 0c's behaviour change (recommended: prune a generated skill directory), the `sha2` and `signal-hook` dependencies (recommended: both), the post-sync trust gate (recommended: the embedded config plus `AGENTSYNC_ALLOW_POST_SYNC`), and ratifying the eight proposed deviations. After approval, Task 0 Step 1.
- Blocker: none. `cargo` is installed at `~/.cargo/bin` but is not on the agent shell's `PATH`; commands ran as `PATH="$HOME/.cargo/bin:$PATH" cargo …`. The agent sandbox denies `openpty`, so the `script(1)` checks ran outside it, and it denies writes under `.claude/`, which Task 11 Step 3 needs.
