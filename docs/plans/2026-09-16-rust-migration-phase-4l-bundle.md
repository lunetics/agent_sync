# Rust Migration Phase 4l: Native `export` and `import`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync export` and `agentsync import` so the binary answers them byte for byte like `cmd_export` in `lib/helpers/export.sh` and `cmd_import` in `lib/helpers/import.sh`: the source-path resolution with its layout detection and config overrides, the contents report, the `tar` archive and its size line, the three import sources (a GitHub archive through `curl`, a `tar.gz`, a directory), the target filter, the change preview with its counters, the confirmation on a terminal, and the copy. Four Bash bugs the reference turned up are fixed first, and the two commands, which had no tests, get a bats file.

**Architecture:** `src/cli/bundle.rs` holds both commands: `resolve_sources` (`_resolve_source_paths`), `export`, and `import` with `diff_file`, `diff_dir`, `filter_targets`, `find_ai_src`, and `github_segments`. Archives go through the `tar` executable and downloads through `curl`, as Bash ran them, so the binary adds no archive or HTTP dependency; a `Scratch` directory under the system temp dir stands in for the run directory and is removed on drop. `main` hands `import` the logical working directory, whether stdin is a terminal, and a line reader for the `Proceed? [Y/n]` answer. The seam stays the CLI process boundary: the new `tests/bundle.bats` under `AGENTSYNC_NATIVE=1`, two parity fixtures, a 48-scenario reference harness with a `curl` stand-in, and a probe.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-4k-add.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `export` writes the one archive it names; `import` writes below the detected source base and `.ai/agent_sync.yaml`, and its scratch directory is removed; `--dry-run` writes nothing.
- No binary ships to users; without a binary every command runs in Bash. Four Bash changes, Tasks 1–4, each in its own commit with a regression test in the new `tests/bundle.bats`; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations and the archive bytes, whose gzip header carries the creation time.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state. The binary spawns `tar` and `curl` and nothing else new.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from the fixed Bash on 2026-09-16: `phase4l/bundle_reference.sh` (48 scenarios, 1150 lines with archive listings and trees, through `phase4l/fake_curl.sh`) and `phase4l/tiny_probe.sh` (stdout and stderr apart on a four-file project). The scripts are reproduced in Task 5 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-16 under the maintainer's standing instruction to run Phase 4 to its close; each was checked against Bash before the call.

1. **Fix four Bash bugs before porting.** Each was reproduced on the committed engine and each regression test fails there.
   - **`import` is unusable.** The dispatcher loads `import.sh` alone, but every real source reaches `_BUNDLE_CONFIG` and `_resolve_source_paths`, which live in `export.sh`, so the run dies with `import.sh: line 95: _BUNDLE_CONFIG: unbound variable` and, through the EXIT trap, exits 0. **Fix:** `_need yaml export import` (Task 1).
   - **A source without `.ai` exits silently.** `src_root=$(_import_find_ai_src …)` returns 1 under `set -e`, so the run ends with status 1 before the message meant for it. **Fix:** `|| src_root=""` (Task 2).
   - **A relative `--output` is sized from the wrong directory.** The archive is written from the project root but `stat` runs from the working directory, so `export -o rel.tgz` from a subdirectory prints `(? B)`. **Fix:** size the archive from the project root (Task 3).
   - **An empty selection dies under Bash 3.2.** `"${changes[@]}"`, `"${targets[@]}"`, and `"${filtered[@]}"` are unbound when the array is empty, so `--only` with no match, or a run where only the config changed, dies with `changes[@]: unbound variable`. **Fix:** the `${arr[@]+"${arr[@]}"}` idiom `backup.sh` already uses (Task 4).
   Alternative: port the behaviour as is; the first and fourth make the command useless, the second is the silent exit the skill names, the third prints a wrong size, so none is offered as a quirk.
2. **Archives and downloads through the executables Bash used.** The crate has no tar, gzip, or HTTP dependency and the constraint forbids adding one; `tar -czf`, `tar -xzf`, and `curl -sfL --max-time 30 -o` are spawned with the same arguments, their stderr passing through as it did, so `tar: Failed to open …` reads the same. Alternative: the `tar`, `flate2`, and `ureq` crates; rejected by the no-new-dependency constraint and because the reference's archives would then differ.
3. **Quirks and deviations.** Record as known quirks 50–51: `import` strips `.git` before a trailing `/`, so `https://github.com/user/repo.git/` downloads `repo.git`; a directory import copies `.ai/` alone, so a `source:` override pointing elsewhere in the source project is not carried. Record as an accepted deviation the archive bytes: two `tar -czf` runs on the same files differ in the gzip header's time, so the parity fixtures compare the report and the archive's listing, never the bytes. `import`'s scratch directory lives under the system temp dir instead of the run directory; nothing observable changes. **Recommended:** as listed.

## Module closure

```text
lib/helpers/export.sh            4-9      _BUNDLE_* constants (Task 1 loads them for import)
                                 15-84    _resolve_source_paths
                                 87-94    _count_files_recursive
                                 98-221   cmd_export (Task 3 at 201-206)
                                 223-241  _export_usage
lib/helpers/import.sh            5-238    cmd_import (Task 2 at 76; Task 4 at 114-115, 125, 134, 171, 205)
                                 242-278  _import_diff_file, _import_diff_dir
                                 282-292  _import_copy_dir
                                 296-302  _import_is_github_url, _import_is_archive
                                 306-402  _import_from_github, _import_from_archive, _import_from_directory
                                 406-431  _import_find_ai_src
                                 435-462  _import_usage
bin/agentsync.sh                 280      _NATIVE_COMMANDS; 380-381 export's and import's _need lists (Task 1)
```

Reused: `paths::{logical_root, normalize}`, `yaml_subset::value`, `style`, `cli::customize::put`. The project root is `AGENTSYNC_REPO_ROOT` or the working directory, as both commands took it, with no discovery.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
printf 'native_parity bash=%s\n' "$(AGENTSYNC_NATIVE=0 bats --tap tests/native_parity.bats | grep -c '^not ok')"
ls tests/bundle.bats 2>/dev/null || echo "no bundle tests yet"
```

Expected: the plan's latest commit; `271 passed`, `0 passed`, `11 passed`, `1 passed`; `0`, run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses; `no bundle tests yet`.

---

### Task 1: `import` loads the bundle constants

**Files:**
- Modify: `bin/agentsync.sh` (the `import` arm)
- Create: `tests/bundle.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing tests**

Create `tests/bundle.bats`:

```bash
#!/usr/bin/env bats
# Tests for agentsync export and import: a bundle round trip, a directory
# source, and a GitHub archive served by a curl stand-in on PATH.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }
setup() { clone_seed; }
teardown() { teardown_test_project; }

# Put a `curl` on PATH that serves $TEST_PROJECT/github/<owner>_<repo>-<branch>.tar.gz
# for the archive URL import builds, and fails like `curl -f` otherwise.
github_stub() {
    mkdir -p stub github
    cat > stub/curl <<'EOF'
#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
name="${url##*/archive/refs/heads/}"
repo="${url#https://github.com/}"; repo="${repo%%/archive/*}"
file="$FAKE_GITHUB_DIR/${repo//\//_}-$name"
[ -f "$file" ] || exit 22
cp "$file" "$out"
EOF
    chmod +x stub/curl
    export FAKE_GITHUB_DIR="$TEST_PROJECT/github"
    export PATH="$TEST_PROJECT/stub:$PATH"
}

# An archive as GitHub serves one: the repository under <repo>-<branch>/.
github_archive() {
    local owner_repo="$1" branch="$2" top
    top="${owner_repo#*/}-$branch"
    mkdir -p "gh/$top/.ai/src/rules"
    printf '# From %s\n' "$branch" > "gh/$top/.ai/src/AGENTS.md"
    printf '# GH rule\n' > "gh/$top/.ai/src/rules/gh.md"
    (cd gh && tar -czf "$FAKE_GITHUB_DIR/${owner_repo//\//_}-$branch.tar.gz" "$top")
}

@test "export writes the bundle and lists its contents" {
    run run_agentsync export
    [ "$status" -eq 0 ]
    grep -qF -- "AGENTS.md" <<<"$output"
    grep -qF -- "rules/ (" <<<"$output"
    grep -qF -- "Exported!" <<<"$output"
    [ -f agentsync-bundle.tar.gz ]
    tar -tzf agentsync-bundle.tar.gz | grep -q '^.ai/src/AGENTS.md$'
    tar -tzf agentsync-bundle.tar.gz | grep -q '^.ai/agent_sync.yaml$'
}

@test "export --dry-run writes nothing" {
    run run_agentsync export --dry-run
    [ "$status" -eq 0 ]
    grep -qF -- "Dry run" <<<"$output"
    [ ! -e agentsync-bundle.tar.gz ]
}

@test "export sizes a relative archive from the project root" {
    mkdir -p sub
    run bash -c "cd sub && AGENTSYNC_REPO_ROOT='$TEST_PROJECT' AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' export -o rel.tgz"
    [ "$status" -eq 0 ]
    [ -f rel.tgz ]
    grep -qF -- "rel.tgz (" <<<"$output"
    ! grep -qF -- "(? B)" <<<"$output"
}

@test "export fails without .ai" {
    rm -rf .ai
    run run_agentsync export
    [ "$status" -eq 1 ]
    grep -qF -- "No .ai/ directory found" <<<"$output"
}

@test "import copies a bundle into a fresh project" {
    run_agentsync export -o bundle.tgz >/dev/null
    mkdir -p fresh
    mv bundle.tgz fresh/
    run bash -c "cd fresh && AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' import bundle.tgz"
    [ "$status" -eq 0 ]
    grep -qF -- "Imported!" <<<"$output"
    [ -f fresh/.ai/src/AGENTS.md ]
    [ -f fresh/.ai/agent_sync.yaml ]
    cmp -s .ai/src/rules/core.md fresh/.ai/src/rules/core.md
}

@test "import reports an up-to-date project" {
    run_agentsync export -o bundle.tgz >/dev/null
    run run_agentsync import bundle.tgz
    [ "$status" -eq 0 ]
    grep -qF -- "Already up to date!" <<<"$output"
}

@test "import --dry-run previews without writing" {
    run_agentsync export -o bundle.tgz >/dev/null
    mkdir -p fresh
    mv bundle.tgz fresh/
    run bash -c "cd fresh && AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' import bundle.tgz --dry-run"
    [ "$status" -eq 0 ]
    grep -qF -- "Dry run" <<<"$output"
    [ ! -e fresh/.ai ]
}

@test "import --only limits the targets" {
    mkdir -p other/.ai/src/rules other/.ai/src/skills/new
    printf '# Other rule\n' > other/.ai/src/rules/other.md
    printf '# New skill\n' > other/.ai/src/skills/new/SKILL.md
    run run_agentsync import other --only rules
    [ "$status" -eq 0 ]
    [ -f .ai/src/rules/other.md ]
    [ ! -e .ai/src/skills/new ]
}

@test "import --only that matches nothing reports an up-to-date project" {
    run_agentsync export -o bundle.tgz >/dev/null
    run run_agentsync import bundle.tgz --only bogus
    [ "$status" -eq 0 ]
    grep -qF -- "Already up to date!" <<<"$output"
}

@test "import --only previews a config-only change" {
    mkdir -p other/.ai/src/skills/new
    printf '# New skill\n' > other/.ai/src/skills/new/SKILL.md
    printf 'outputs: committed\n' > other/.ai/agent_sync.yaml
    run run_agentsync import other --only rules --dry-run
    [ "$status" -eq 0 ]
    grep -qF -- "~ agent_sync.yaml (update)" <<<"$output"
    grep -qF -- "Summary: 0 new, 1 updated, 0 unchanged" <<<"$output"
    ! grep -qF -- "unbound variable" <<<"$output"
}

@test "import from a directory updates changed files" {
    mkdir -p other/.ai/src/rules
    printf '# Replaced core\n' > other/.ai/src/rules/core.md
    run run_agentsync import other --force
    [ "$status" -eq 0 ]
    grep -qF -- "1 updated" <<<"$output"
    grep -q '^# Replaced core$' .ai/src/rules/core.md
}

@test "import refuses a source without .ai" {
    mkdir -p plain/docs
    printf 'x\n' > plain/docs/readme.md
    run run_agentsync import plain
    [ "$status" -eq 1 ]
    grep -qF -- "No .ai/src/ (or .ai/) directory found in source." <<<"$output"
}

@test "import rejects an unrecognized source" {
    run run_agentsync import nothing.txt
    [ "$status" -eq 1 ]
    grep -qF -- "Cannot recognize source" <<<"$output"
}

@test "import downloads a GitHub archive through curl" {
    github_stub
    github_archive user/repo main
    run run_agentsync import https://github.com/user/repo --force
    [ "$status" -eq 0 ]
    grep -qF -- "Downloading user/repo (branch: main)" <<<"$output"
    grep -qF -- "Downloaded." <<<"$output"
    [ -f .ai/src/rules/gh.md ]
}

@test "import falls back to master when main is missing" {
    github_stub
    github_archive user/repo2 master
    run run_agentsync import https://github.com/user/repo2 --force
    [ "$status" -eq 0 ]
    grep -qF -- "trying 'master'" <<<"$output"
    grep -q '^# From master$' .ai/src/AGENTS.md
}

@test "import reports a branch that cannot be downloaded" {
    github_stub
    run run_agentsync import https://github.com/user/repo --branch nope
    [ "$status" -eq 1 ]
    grep -qF -- "Failed to download branch 'nope'." <<<"$output"
}
```

Run: `bats --tap tests/bundle.bats | grep -c '^not ok'`
Expected: `11` — every import case but `import rejects an unrecognized source` and `import reports a branch that cannot be downloaded`, plus `export sizes a relative archive from the project root`, fail on the committed engine. The assertions are `grep -qF … <<<"$output"`, not `[[ ]]`: after a `run` whose child died on `set -u` at top level, a failing `[[ ]]` does not fail the test under Bash 3.2 and bats 1.13, while `grep` and `[ ]` do.

- [x] **Step 2: Load export.sh for import**

In `bin/agentsync.sh`, change the `import` arm to `import)        _need yaml export import;                       shift; cmd_import "$@" ;;`.

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/bundle.bats | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
```

Expected: `4` (`export sizes a relative archive from the project root`, `import refuses a source without .ai`, `import --only that matches nothing reports an up-to-date project`, `import --only previews a config-only change`); ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add bin/agentsync.sh tests/bundle.bats docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "fix(import): load the bundle constants export defines"
```

---

### Task 2: A source without `.ai` is named

**Files:**
- Modify: `lib/helpers/import.sh` (`cmd_import`, the `_import_find_ai_src` call)

**Interfaces:** none.

- [x] **Step 1: Confirm the failing test**

Run: `bats --tap -f 'source without .ai' tests/bundle.bats`
Expected: `not ok 1 import refuses a source without .ai` (status 1 and no message on the committed engine).

- [x] **Step 2: Keep the run alive past the lookup**

In `cmd_import`, replace `src_root=$(_import_find_ai_src "$tmp_dir")` with:

```bash
    src_root=$(_import_find_ai_src "$tmp_dir") || src_root=""
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/bundle.bats | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 lib/helpers/import.sh
```

Expected: `3`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/import.sh docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "fix(import): report a source without .ai"
```

---

### Task 3: A relative archive is sized from the project root

**Files:**
- Modify: `lib/helpers/export.sh` (`cmd_export`, the size lookup)

**Interfaces:** none.

- [x] **Step 1: Confirm the failing test**

Run: `bats --tap -f 'relative archive' tests/bundle.bats`
Expected: `not ok 1 export sizes a relative archive from the project root` (`(? B)` on the committed engine).

- [x] **Step 2: Size the archive where tar wrote it**

In `cmd_export`, replace the `local size` block through its `fi` with:

```bash
    # A relative --output was created from repo_root, so it is sized from there.
    local archive="$output"
    [[ "$output" == /* ]] || archive="$repo_root/$output"
    local size
    if [[ "$(uname)" == "Darwin" ]]; then
        size=$(stat -f%z "$archive" 2>/dev/null || echo "?")
    else
        size=$(stat -c%s "$archive" 2>/dev/null || echo "?")
    fi
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/bundle.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/export.sh
```

Expected: `14`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/export.sh docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "fix(export): size a relative archive from the project root"
```

---

### Task 4: `import` survives an empty selection

**Files:**
- Modify: `lib/helpers/import.sh` (`cmd_import`, the array expansions)

**Interfaces:** none.

- [x] **Step 1: Confirm the failing tests**

Run: `bats --tap -f 'matches nothing|config-only' tests/bundle.bats`
Expected: `not ok 1 import --only that matches nothing reports an up-to-date project` and `not ok 2 import --only previews a config-only change` (`changes[@]: unbound variable` with Tasks 1–3 in place).

- [x] **Step 2: Expand the arrays as Bash 3.2 allows**

In `cmd_import`, replace `"${targets[@]}"` (three places), `"${selected_arr[@]}"`, `"${filtered[@]}"`, and `"${changes[@]}"` with `"${targets[@]+"${targets[@]}"}"`, `"${selected_arr[@]+"${selected_arr[@]}"}"`, `"${filtered[@]+"${filtered[@]}"}"`, and `"${changes[@]+"${changes[@]}"}"`.

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/bundle.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/import.sh
```

Expected: `16`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/import.sh docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "fix(import): survive an empty selection under Bash 3.2"
```

---

### Task 5: Port `export` and `import`

**Files:**
- Create: `src/cli/bundle.rs`
- Modify: `src/cli/mod.rs`, `src/main.rs`, `bin/agentsync.sh`
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/cli/bundle.rs
pub struct Env<'a> {
    pub cwd: String,
    pub interactive: bool,
    pub read_line: &'a mut dyn FnMut() -> String,
}
pub fn export(args: &[String], root: &str, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
pub fn import(args: &[String], root: &str, style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
```

- [x] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── export and import ────────────────────────────────────────────────────────
# A gzip stream carries its creation time, so the archive bytes are never
# compared: export is checked on its report and the archive's listing, import
# on the tree it leaves behind. GitHub downloads go through a curl stand-in.

_parity_curl_stub() {
    mkdir -p "$BATS_TEST_TMPDIR/stub" "$BATS_TEST_TMPDIR/github"
    cat > "$BATS_TEST_TMPDIR/stub/curl" <<'EOF'
#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
name="${url##*/archive/refs/heads/}"
repo="${url#https://github.com/}"; repo="${repo%%/archive/*}"
file="$FAKE_GITHUB_DIR/${repo//\//_}-$name"
[ -f "$file" ] || exit 22
cp "$file" "$out"
EOF
    chmod +x "$BATS_TEST_TMPDIR/stub/curl"
    export FAKE_GITHUB_DIR="$BATS_TEST_TMPDIR/github"
    export PATH="$BATS_TEST_TMPDIR/stub:$PATH"
}

@test "parity: export previews, bundles, and refuses like Bash" {
    assert_parity export --help
    assert_parity export --bogus
    assert_parity export -o
    assert_parity export --dry-run
    assert_parity export -o bundles/missing.tgz
    mkdir -p .ai/src/settings custom/rules
    printf '{"legacy": true}\n' > .ai/src/settings/cursor.json
    printf '# Custom\n' > custom/rules/x.md
    printf 'source:\n  rules: custom/rules\n' > .ai/agent_sync.yaml
    assert_parity export --dry-run
    assert_parity export -o bundle.tgz
    tar -tzf bundle.tgz | grep -q '^custom/rules/x.md$'
    mv .ai/agent_sync.yaml agent_sync.yaml
    assert_parity export --dry-run
    rm -rf .ai
    assert_parity export
}

@test "parity: import brings a bundle, a directory, and a GitHub archive in like Bash" {
    assert_tree_parity import --help
    assert_tree_parity import
    assert_tree_parity import --bogus
    assert_tree_parity import a b
    assert_tree_parity import --only
    assert_tree_parity import nothing.txt
    mkdir -p "$BATS_TEST_TMPDIR/plain/docs"
    printf 'x\n' > "$BATS_TEST_TMPDIR/plain/docs/readme.md"
    assert_tree_parity import "$BATS_TEST_TMPDIR/plain"
    run_agentsync export -o "$BATS_TEST_TMPDIR/bundle.tgz" >/dev/null
    assert_tree_parity import "$BATS_TEST_TMPDIR/bundle.tgz"
    printf '# Edited core\n' > .ai/src/rules/core.md
    printf '# Added\n' > .ai/src/rules/added.md
    assert_tree_parity import "$BATS_TEST_TMPDIR/bundle.tgz" --dry-run
    assert_tree_parity import "$BATS_TEST_TMPDIR/bundle.tgz" --only " rules , bogus "
    assert_tree_parity import "$BATS_TEST_TMPDIR/bundle.tgz"
    mkdir -p "$BATS_TEST_TMPDIR/other/.ai/src/skills/new"
    printf '# New skill\n' > "$BATS_TEST_TMPDIR/other/.ai/src/skills/new/SKILL.md"
    printf 'outputs: committed\n' > "$BATS_TEST_TMPDIR/other/agent_sync.yaml"
    assert_tree_parity import "$BATS_TEST_TMPDIR/other" --force
    _parity_curl_stub
    mkdir -p "$BATS_TEST_TMPDIR/gh/repo-main/.ai/src/rules" "$BATS_TEST_TMPDIR/gh/repo2-master/.ai/src"
    printf '# From GitHub\n' > "$BATS_TEST_TMPDIR/gh/repo-main/.ai/src/AGENTS.md"
    printf '# GH rule\n' > "$BATS_TEST_TMPDIR/gh/repo-main/.ai/src/rules/gh.md"
    printf '# From master\n' > "$BATS_TEST_TMPDIR/gh/repo2-master/.ai/src/AGENTS.md"
    (cd "$BATS_TEST_TMPDIR/gh" && tar -czf "$FAKE_GITHUB_DIR/user_repo-main.tar.gz" repo-main && tar -czf "$FAKE_GITHUB_DIR/user_repo2-master.tar.gz" repo2-master)
    assert_tree_parity import https://github.com/user/repo --force
    assert_tree_parity import https://github.com/user/repo2 --dry-run
    assert_tree_parity import https://github.com/user/repo3
    assert_tree_parity import https://github.com/user/repo/tree/develop
    assert_tree_parity import https://github.com/user
    rm -rf .ai
    assert_tree_parity import "$BATS_TEST_TMPDIR/bundle.tgz"
}
```

Run, outside the sandbox: `AGENTSYNC_NATIVE=0 bats --tap tests/bundle.bats | grep -c '^ok'`; `bats --tap -f 'parity: export|parity: import' tests/native_parity.bats`
Expected: `16`; `ok 1` and `ok 2` (the native side still runs Bash until Step 3 lists the commands).

- [x] **Step 2: Write the failing tests**

Create `src/cli/bundle.rs` with the tests module, and add `pub mod bundle;` to `src/cli/mod.rs` after `adopt`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_filters_the_targets_in_target_order() {
        let targets: Vec<&'static str> = {
            let mut all = vec!["AGENTS.md"];
            all.extend(DIR_TARGETS);
            all
        };
        assert_eq!(
            filter_targets(&targets, " AGENTS , bogus "),
            vec!["AGENTS.md"]
        );
        assert_eq!(
            filter_targets(&targets, "skills,rules"),
            vec!["rules", "skills"]
        );
        assert_eq!(
            filter_targets(&targets, "AGENTS.md,tools"),
            vec!["AGENTS.md", "tools"]
        );
        assert!(filter_targets(&targets, "nothing").is_empty());
    }

    #[test]
    fn github_urls_are_recognised_like_import_is_github_url() {
        assert_eq!(
            github_segments("https://github.com/user/repo"),
            Some(("user", "repo"))
        );
        assert_eq!(
            github_segments("https://www.github.com/user/repo/tree/x"),
            Some(("user", "repo"))
        );
        assert_eq!(
            github_segments("http://github.com/a/b.git"),
            Some(("a", "b.git"))
        );
        assert_eq!(github_segments("https://github.com/user"), None);
        assert_eq!(github_segments("https://gitlab.com/a/b"), None);
        assert_eq!(github_segments("bundle.tar.gz"), None);
    }

    #[test]
    fn sizes_read_like_the_exported_line() {
        assert_eq!(human_size(Some(543)), "543 B");
        assert_eq!(human_size(Some(46_000)), "44 KB");
        assert_eq!(human_size(Some(2_000_000)), "1 MB");
        assert_eq!(human_size(None), "? B");
    }

    #[cfg(unix)]
    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The four-file project of `tiny_probe.sh`.
    #[cfg(unix)]
    fn tiny_project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        write(dir.path(), ".ai/src/AGENTS.md", "# Agents\n");
        write(dir.path(), ".ai/src/rules/a.md", "# A rule\n");
        write(dir.path(), ".ai/src/skills/x/SKILL.md", "# Skill\n");
        write(dir.path(), "custom/cmds/c.md", "# Command\n");
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "source:\n  commands: custom/cmds\n",
        );
        (dir, root)
    }

    #[cfg(unix)]
    #[test]
    fn sources_are_resolved_like_resolve_source_paths() {
        let (dir, root) = tiny_project();
        let sources = resolve_sources(&root);
        assert_eq!(sources.base, ".ai/src");
        assert_eq!(sources.agents, ".ai/src/AGENTS.md");
        assert_eq!(
            sources.dirs,
            vec![
                ("rules", ".ai/src/rules".to_string()),
                ("skills", ".ai/src/skills".to_string()),
                ("commands", "custom/cmds".to_string()),
                ("agents", String::new()),
                ("settings", String::new()),
                ("mcp", String::new()),
                ("hooks", String::new()),
                ("tools", String::new()),
            ]
        );
        let legacy = tempfile::tempdir().unwrap();
        write(legacy.path(), ".ai/AGENTS.md", "# Old\n");
        write(legacy.path(), ".ai/rules/r.md", "# R\n");
        let sources = resolve_sources(&legacy.path().to_string_lossy());
        assert_eq!(sources.base, ".ai");
        assert_eq!(sources.agents, ".ai/AGENTS.md");
        assert_eq!(sources.dirs[0], ("rules", ".ai/rules".to_string()));
        assert_eq!(
            resolve_sources(&dir.path().join("custom").to_string_lossy()).base,
            ""
        );
    }

    #[cfg(unix)]
    fn run_export(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = export(&args, root, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    fn run_import(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut read_line = || String::new();
        let mut env = Env {
            cwd: root.to_string(),
            interactive: false,
            read_line: &mut read_line,
        };
        let status = import(&args, root, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_dry_run_export_lists_the_sources_like_cmd_export() {
        let (_dir, root) = tiny_project();
        let (status, out, err) = run_export(&root, &["--dry-run"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Export\n\n  Contents:\n    • AGENTS.md\n    • rules/ (1 files)\n    • skills/ (1 files)\n    • commands/ (1 files)\n    • agent_sync.yaml\n\n  Dry run — no files written.\n  Would create: {root}/agentsync-bundle.tar.gz\n\n"
            )
        );
        let (status, out, err) = run_export(&root, &["--bogus"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert!(err.starts_with("Error: Unknown option: --bogus\n\n  agentsync export — bundle"));
        let (status, _, err) = run_export(&root, &["-o"]);
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: --output requires a path\n")
        );
        let empty = tempfile::tempdir().unwrap();
        let empty_root = empty.path().to_string_lossy().into_owned();
        let (status, _, err) = run_export(&empty_root, &[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            format!("Error: No .ai/ directory found in {empty_root}\nRun agentsync init first.\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_import_copies_then_reports_up_to_date_like_cmd_import() {
        let (_source, source_root) = tiny_project();
        let target = tempfile::tempdir().unwrap();
        let target_root = std::fs::canonicalize(target.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (status, out, err) = run_import(&target_root, &[&source_root]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Import\n\n  Reading from {source_root}...\n  Source: Directory: {source_root}\n\n  Changes:\n    + AGENTS.md (new)\n    ↳ rules (1 new)\n    ↳ skills (1 new)\n    + agent_sync.yaml (new)\n\n  Summary: 4 new, 0 updated, 0 unchanged\n\n  Imported! 4 new, 0 updated files.\n\n  Next steps:\n    1. Review imported files in .ai/src\n    2. Run agentsync sync to distribute to all tools\n\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join(".ai/src/skills/x/SKILL.md")).unwrap(),
            "# Skill\n"
        );
        assert!(!target.path().join(".ai/src/commands").exists());
        let (status, out, _) = run_import(&target_root, &[&source_root]);
        assert_eq!(status, 0);
        assert!(out.ends_with("  Source: Directory: {source_root}\n\n  Already up to date! Nothing to import.\n\n".replace("{source_root}", &source_root).as_str()));
        write(
            Path::new(&source_root),
            ".ai/src/rules/a.md",
            "# A rule, edited\n",
        );
        write(Path::new(&source_root), ".ai/src/rules/b.md", "# B\n");
        let (status, out, _) = run_import(
            &target_root,
            &[&source_root, "--only", "rules", "--dry-run"],
        );
        assert_eq!(status, 0);
        assert!(out.ends_with(
            "  Changes:\n    ↳ rules (1 new, 1 updated)\n\n  Summary: 1 new, 1 updated, 0 unchanged\n\n  Dry run — no files written.\n\n"
        ));
        let (status, out, err) = run_import(&target_root, &["nothing.txt"]);
        assert_eq!((status, out.as_str()), (1, "\n  AgentSync Import\n\n"));
        assert_eq!(
            err,
            "  Error: Cannot recognize source: nothing.txt\n  Expected: GitHub URL, .tar.gz file, or directory path.\n"
        );
    }
}
```

Run: `cargo test cli::bundle 2>&1 | grep -E '^error' | head -3`
Expected: compile errors naming `filter_targets`, `github_segments`, `human_size`, `resolve_sources`, `export`, and `import`.

- [x] **Step 3: Write the implementation**

Prepend to `src/cli/bundle.rs`:

```rust
//! `agentsync export` and `agentsync import`: `cmd_export` of
//! `lib/helpers/export.sh` and `cmd_import` of `lib/helpers/import.sh`, which
//! bundle a project's sources into a `tar.gz` and bring a bundle, a directory,
//! or a GitHub archive back in. Archives go through the `tar` and `curl`
//! executables, as Bash ran them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::customize::put;
use crate::style::Style;
use crate::{Error, yaml_subset};

/// `_BUNDLE_DIR_TARGETS`, in the order `init` creates them.
const DIR_TARGETS: [&str; 8] = [
    "rules", "skills", "commands", "agents", "settings", "mcp", "hooks", "tools",
];
const CONFIG: &str = ".ai/agent_sync.yaml";
const CONFIG_LEGACY: &str = "agent_sync.yaml";

/// What `import` takes from the process.
pub struct Env<'a> {
    /// The logical working directory a relative source is shown against.
    pub cwd: String,
    /// `[[ -t 0 ]]`: the confirmation is asked only when stdin is a terminal.
    pub interactive: bool,
    /// `read -r answer` on stdin.
    pub read_line: &'a mut dyn FnMut() -> String,
}

/// `_resolve_source_paths`: the detected base and every source path, each
/// relative to the project root or as the config spelled it.
#[derive(Debug, Default, PartialEq)]
struct Sources {
    base: String,
    agents: String,
    dirs: Vec<(&'static str, String)>,
}

fn resolve_sources(root: &str) -> Sources {
    let mut config = format!("{root}/{CONFIG}");
    if !Path::new(&config).is_file() {
        config = format!("{root}/{CONFIG_LEGACY}");
    }
    let base = if Path::new(root).join(".ai/src").is_dir() {
        ".ai/src"
    } else if Path::new(root).join(".ai").is_dir() {
        ".ai"
    } else {
        ""
    };
    let mut sources = Sources {
        base: base.to_string(),
        ..Sources::default()
    };
    if !base.is_empty() && Path::new(root).join(base).join("AGENTS.md").is_file() {
        sources.agents = format!("{base}/AGENTS.md");
    }
    sources.dirs = DIR_TARGETS
        .iter()
        .map(|name| {
            let path = if !base.is_empty() && Path::new(root).join(base).join(name).is_dir() {
                format!("{base}/{name}")
            } else {
                String::new()
            };
            (*name, path)
        })
        .collect();
    if let Ok(bytes) = std::fs::read(&config) {
        let text = String::from_utf8_lossy(&bytes);
        let override_of = |key: &str| yaml_subset::value(&text, key);
        let agents = override_of("source.agents");
        if !agents.is_empty() {
            sources.agents = agents;
        }
        for (name, key) in [
            ("rules", "source.rules"),
            ("skills", "source.skills"),
            ("commands", "source.commands"),
            ("agents", "source.subagents"),
            ("tools", "source.tools"),
        ] {
            let value = override_of(key);
            if !value.is_empty()
                && let Some(entry) = sources.dirs.iter_mut().find(|(n, _)| *n == name)
            {
                entry.1 = value;
            }
        }
    }
    sources
}

/// `find <dir> -type f`, recursively; symlinks are not followed.
fn files_below(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
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

fn count_files(dir: &Path) -> usize {
    let mut files = Vec::new();
    files_below(dir, &mut files);
    files.len()
}

fn export_usage(style: &Style) -> String {
    format!(
        "\n  {} — bundle source files into a shareable archive\n\n  {}\n    agentsync export [options]\n\n  {}\n    --output, -o <path>   Output file path (default: ./agentsync-bundle.tar.gz)\n    --dry-run             Preview what would be exported\n    --help, -h            Show this message\n\n  {}\n    agentsync export\n    agentsync export -o my-config.tar.gz\n    agentsync export --dry-run\n\n",
        style.bold("agentsync export"),
        style.green("USAGE"),
        style.green("OPTIONS"),
        style.green("EXAMPLES")
    )
}

/// `stat -f%z` rendered as `cmd_export` prints it.
fn human_size(size: Option<u64>) -> String {
    match size {
        Some(size) if size >= 1_048_576 => format!("{} MB", size / 1_048_576),
        Some(size) if size >= 1024 => format!("{} KB", size / 1024),
        Some(size) => format!("{size} B"),
        None => "? B".to_string(),
    }
}

/// `cmd_export`.
pub fn export(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut output = String::new();
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--output" | "-o" => {
                let Some(value) = args.get(i + 1) else {
                    put(
                        err,
                        format!("{}: --output requires a path\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(1);
                };
                output = value.clone();
                i += 2;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--help" | "-h" => {
                put(out, export_usage(style).as_bytes())?;
                return Ok(0);
            }
            other => {
                put(
                    err,
                    format!("{}: Unknown option: {other}\n", style.red("Error")).as_bytes(),
                )?;
                put(err, export_usage(style).as_bytes())?;
                return Ok(1);
            }
        }
    }
    let sources = resolve_sources(root);
    if sources.base.is_empty() {
        put(
            err,
            format!(
                "{}: No .ai/ directory found in {root}\nRun {} first.\n",
                style.red("Error"),
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if output.is_empty() {
        output = format!("{root}/agentsync-bundle.tar.gz");
    }
    put(
        out,
        format!("\n{}\n\n", style.bold("  AgentSync Export")).as_bytes(),
    )?;

    let mut items: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    if !sources.agents.is_empty() && Path::new(root).join(&sources.agents).is_file() {
        items.push(sources.agents.clone());
        labels.push(
            sources
                .agents
                .rsplit('/')
                .next()
                .unwrap_or(&sources.agents)
                .to_string(),
        );
    }
    for (name, path) in &sources.dirs {
        if path.is_empty() {
            continue;
        }
        let dir = Path::new(root).join(path);
        if !dir.is_dir() {
            continue;
        }
        let count = count_files(&dir);
        if count > 0 {
            items.push(path.clone());
            labels.push(format!("{name}/ ({count} files)"));
        }
    }
    if Path::new(root).join(CONFIG).is_file() {
        items.push(CONFIG.to_string());
        labels.push("agent_sync.yaml".to_string());
    } else if Path::new(root).join(CONFIG_LEGACY).is_file() {
        items.push(CONFIG_LEGACY.to_string());
        labels.push(format!("agent_sync.yaml {}", style.dim("(legacy)")));
    }
    if items.is_empty() {
        put(
            out,
            format!(
                "  {} — source directories are empty.\n\n",
                style.yellow("Nothing to export")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let mut text = format!("  {}\n", style.green("Contents:"));
    for label in &labels {
        text.push_str(&format!("    {} {label}\n", style.dim("•")));
    }
    text.push('\n');
    if dry_run {
        text.push_str(&format!(
            "  {} — no files written.\n  Would create: {}\n\n",
            style.yellow("Dry run"),
            style.cyan(&output)
        ));
        put(out, text.as_bytes())?;
        return Ok(0);
    }
    put(out, text.as_bytes())?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&output)
        .args(&items)
        .current_dir(root)
        .stdin(Stdio::null())
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        put(
            err,
            format!("  {}: Failed to create archive.\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(1);
    }
    let archive = if output.starts_with('/') {
        PathBuf::from(&output)
    } else {
        Path::new(root).join(&output)
    };
    let size = std::fs::metadata(&archive).ok().map(|m| m.len());
    let base_name = output.rsplit('/').next().unwrap_or(&output).to_string();
    put(
        out,
        format!(
            "  {} → {} ({})\n\n  Share this file and import with:\n    {} {}\n\n",
            style.green("Exported!"),
            style.cyan(&output),
            human_size(size),
            style.cyan("agentsync import"),
            style.dim(&base_name)
        )
        .as_bytes(),
    )
    .map(|()| 0)
}

fn import_usage(style: &Style) -> String {
    format!(
        "\n  {} — import config from GitHub, archive, or directory\n\n  {}\n    agentsync import <source> [options]\n\n  {}\n    GitHub URL       https://github.com/user/repo\n    Archive file     path/to/agentsync-bundle.tar.gz\n    Local directory  path/to/project/\n\n  {}\n    --branch, -b <name>   Git branch to download (default: main)\n    --only <targets>      Import only specific targets (comma-separated)\n                          Targets: rules,skills,commands,agents,settings,mcp,hooks,tools\n    --force               Overwrite without confirmation\n    --dry-run             Preview changes without writing\n    --help, -h            Show this message\n\n  {}\n    agentsync import https://github.com/user/repo\n    agentsync import https://github.com/user/repo/tree/develop\n    agentsync import agentsync-bundle.tar.gz\n    agentsync import ../other-project/\n    agentsync import https://github.com/user/repo --only rules,skills\n    agentsync import bundle.tar.gz --dry-run\n\n",
        style.bold("agentsync import"),
        style.green("USAGE"),
        style.green("SOURCES"),
        style.green("OPTIONS"),
        style.green("EXAMPLES")
    )
}

/// `_import_is_github_url`: `^https?://(www\.)?github\.com/[^/]+/[^/]+`.
fn github_segments(source: &str) -> Option<(&str, &str)> {
    let rest = source
        .strip_prefix("https://")
        .or_else(|| source.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;
    let (owner, rest) = rest.split_once('/')?;
    let repo = rest.split('/').next().unwrap_or("");
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

/// A scratch directory under the system temp dir, removed on drop as the run
/// directory was.
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self, Error> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("agentsync-import.{}.{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `cp -R <src>/. <dest>/`: every entry below `src`, directories made as met.
fn copy_tree(src: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let entries = std::fs::read_dir(src).map_err(|e| Error::io(src, e))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from).map_err(|e| Error::io(&from, e))?;
        if meta.is_dir() {
            copy_tree(&from, &to)?;
        } else if meta.is_file() {
            std::fs::copy(&from, &to).map_err(|e| Error::io(&from, e))?;
        }
    }
    Ok(())
}

/// `tar -xzf <archive> -C <dir>`, tar's own diagnostics passing through.
fn extract(archive: &Path, into: &Path) -> bool {
    Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `curl -sfL --max-time 30 -o <file> <url>`, silenced as Bash silenced it.
fn download(url: &str, to: &Path) -> bool {
    Command::new("curl")
        .args(["-sfL", "--max-time", "30", "-o"])
        .arg(to)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn curl_on_path() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("curl").is_file()))
}

/// `_import_find_ai_src`: `.ai/src` over `.ai`, directly or one level down.
fn find_ai_src(search_root: &Path) -> Option<PathBuf> {
    let direct = |dir: &Path| -> Option<PathBuf> {
        let src = dir.join(".ai/src");
        if src.is_dir() {
            return Some(src);
        }
        let ai = dir.join(".ai");
        ai.is_dir().then_some(ai)
    };
    if let Some(found) = direct(search_root) {
        return Some(found);
    }
    let mut names: Vec<String> = std::fs::read_dir(search_root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .find_map(|name| direct(&search_root.join(name)))
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `cp <src> <dest>`: an existing destination keeps its mode, a new one takes
/// the source's.
fn copy_file(src: &Path, dest: &Path) -> Result<(), Error> {
    if dest.is_file() {
        let bytes = std::fs::read(src).map_err(|e| Error::io(src, e))?;
        std::fs::write(dest, bytes).map_err(|e| Error::io(dest, e))
    } else {
        std::fs::copy(src, dest)
            .map(|_| ())
            .map_err(|e| Error::io(src, e))
    }
}

enum Change {
    New(String),
    Update(String),
    Dir(String),
}

#[derive(Default)]
struct Counts {
    new: usize,
    updated: usize,
    skipped: usize,
}

/// `_import_diff_file`.
fn diff_file(src: &Path, dest: &Path, label: &str, changes: &mut Vec<Change>, counts: &mut Counts) {
    if dest.is_file() {
        if same_bytes(src, dest) {
            counts.skipped += 1;
        } else {
            changes.push(Change::Update(label.to_string()));
            counts.updated += 1;
        }
    } else {
        changes.push(Change::New(label.to_string()));
        counts.new += 1;
    }
}

/// `_import_diff_dir`.
fn diff_dir(src: &Path, dest: &Path, label: &str, changes: &mut Vec<Change>, counts: &mut Counts) {
    let (mut new, mut updated, mut skipped) = (0usize, 0usize, 0usize);
    let mut files = Vec::new();
    files_below(src, &mut files);
    for file in files {
        let rel = file.strip_prefix(src).unwrap_or(&file);
        let target = dest.join(rel);
        if target.is_file() {
            if same_bytes(&file, &target) {
                skipped += 1;
            } else {
                updated += 1;
            }
        } else {
            new += 1;
        }
    }
    if new + updated > 0 {
        let mut detail = Vec::new();
        if new > 0 {
            detail.push(format!("{new} new"));
        }
        if updated > 0 {
            detail.push(format!("{updated} updated"));
        }
        if skipped > 0 {
            detail.push(format!("{skipped} unchanged"));
        }
        changes.push(Change::Dir(format!("{label} ({})", detail.join(", "))));
        counts.new += new;
        counts.updated += updated;
        counts.skipped += skipped;
    } else {
        counts.skipped += skipped;
    }
}

/// `--only`: the targets the comma list names, in target order; `AGENTS`
/// names `AGENTS.md`.
fn filter_targets(targets: &[&'static str], only: &str) -> Vec<&'static str> {
    let selected: Vec<&str> = only
        .split('\n')
        .next()
        .unwrap_or("")
        .split(',')
        .map(|item| {
            item.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'))
        })
        .collect();
    targets
        .iter()
        .copied()
        .filter(|target| {
            selected.iter().any(|item| {
                *target == *item || target.strip_suffix(".md").unwrap_or(target) == *item
            })
        })
        .collect()
}

/// `cmd_import`.
pub fn import(
    args: &[String],
    root: &str,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut source = String::new();
    let mut dry_run = false;
    let mut force = false;
    let mut only = String::new();
    let mut branch = String::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--force" => {
                force = true;
                i += 1;
            }
            "--only" | "--branch" | "-b" => {
                let Some(value) = args.get(i + 1) else {
                    let flag = if arg == "--only" {
                        "--only"
                    } else {
                        "--branch"
                    };
                    put(
                        err,
                        format!("{}: {flag} requires a value\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(1);
                };
                if arg == "--only" {
                    only = value.clone();
                } else {
                    branch = value.clone();
                }
                i += 2;
            }
            "--help" | "-h" => {
                put(out, import_usage(style).as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: Unknown option: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                put(err, import_usage(style).as_bytes())?;
                return Ok(1);
            }
            positional => {
                if source.is_empty() {
                    source = positional.to_string();
                } else {
                    put(
                        err,
                        format!(
                            "{}: Unexpected argument: {positional}\n",
                            style.red("Error")
                        )
                        .as_bytes(),
                    )?;
                    return Ok(1);
                }
                i += 1;
            }
        }
    }
    if source.is_empty() {
        put(
            err,
            format!("{}: No source specified.\n", style.red("Error")).as_bytes(),
        )?;
        put(err, import_usage(style).as_bytes())?;
        return Ok(1);
    }
    put(
        out,
        format!("\n{}\n\n", style.bold("  AgentSync Import")).as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let scratch = Scratch::create()?;
    let tmp = scratch.0.as_path();
    let error = style.red("Error");

    let label;
    if let Some((owner, repo)) = github_segments(&source) {
        label = format!("GitHub: {source}");
        if !curl_on_path() {
            put(
                err,
                format!("  {error}: curl is required for GitHub import.\n").as_bytes(),
            )?;
            return Ok(1);
        }
        let url = source.strip_suffix(".git").unwrap_or(&source);
        let url = url.strip_suffix('/').unwrap_or(url);
        let (owner, repo) = github_segments(url).unwrap_or((owner, repo));
        let repo_path = format!("{owner}/{repo}");
        if branch.is_empty()
            && let Some((_, after)) = url.split_once("/tree/")
        {
            branch = after.split('/').next().unwrap_or("").to_string();
        }
        if branch.is_empty() {
            branch = "main".to_string();
        }
        put(
            out,
            format!(
                "  Downloading {} (branch: {branch})...\n",
                style.cyan(&repo_path)
            )
            .as_bytes(),
        )?;
        out.flush().map_err(|e| Error::io("<stdout>", e))?;
        let archive = tmp.join("repo.tar.gz");
        let archive_url = |branch: &str| {
            format!("https://github.com/{repo_path}/archive/refs/heads/{branch}.tar.gz")
        };
        if !download(&archive_url(&branch), &archive) {
            if branch == "main" {
                put(
                    out,
                    format!(
                        "  {}\n",
                        style.dim("Branch 'main' not found, trying 'master'...")
                    )
                    .as_bytes(),
                )?;
                out.flush().map_err(|e| Error::io("<stdout>", e))?;
                branch = "master".to_string();
                if !download(&archive_url(&branch), &archive) {
                    put(
                        err,
                        format!(
                            "  {error}: Failed to download repository.\n  Check the URL and your network connection.\n"
                        )
                        .as_bytes(),
                    )?;
                    return Ok(1);
                }
            } else {
                put(
                    err,
                    format!("  {error}: Failed to download branch '{branch}'.\n").as_bytes(),
                )?;
                return Ok(1);
            }
        }
        if !extract(&archive, tmp) {
            put(
                err,
                format!("  {error}: Failed to extract archive.\n").as_bytes(),
            )?;
            return Ok(1);
        }
        put(
            out,
            format!("  {}\n", style.green("Downloaded.")).as_bytes(),
        )?;
    } else if Path::new(&source).is_file()
        && (source.ends_with(".tar.gz") || source.ends_with(".tgz"))
    {
        let base_name = source.rsplit('/').next().unwrap_or(&source).to_string();
        label = format!("Archive: {base_name}");
        put(
            out,
            format!("  Extracting {}...\n", style.cyan(&base_name)).as_bytes(),
        )?;
        out.flush().map_err(|e| Error::io("<stdout>", e))?;
        if !extract(Path::new(&source), tmp) {
            put(
                err,
                format!("  {error}: Failed to extract archive.\n").as_bytes(),
            )?;
            return Ok(1);
        }
        put(out, format!("  {}\n", style.green("Extracted.")).as_bytes())?;
    } else if Path::new(&source).is_dir() {
        label = format!("Directory: {source}");
        let Ok(canonical) = std::fs::canonicalize(&source) else {
            put(
                err,
                format!("  {error}: Cannot access directory: {source}\n").as_bytes(),
            )?;
            return Ok(1);
        };
        let shown = crate::paths::normalize(&if source.starts_with('/') {
            source.clone()
        } else {
            format!("{}/{source}", env.cwd)
        });
        let shown = if std::fs::canonicalize(&shown).ok().as_deref() == Some(canonical.as_path()) {
            shown
        } else {
            canonical.to_string_lossy().into_owned()
        };
        put(
            out,
            format!("  Reading from {}...\n", style.cyan(&shown)).as_bytes(),
        )?;
        let src_dir = Path::new(&shown);
        if src_dir.join(".ai").is_dir() {
            copy_tree(&src_dir.join(".ai"), &tmp.join(".ai"))?;
        }
        if !tmp.join(CONFIG).is_file() && src_dir.join(CONFIG_LEGACY).is_file() {
            std::fs::create_dir_all(tmp.join(".ai")).map_err(|e| Error::io(tmp, e))?;
            std::fs::copy(src_dir.join(CONFIG_LEGACY), tmp.join(CONFIG))
                .map_err(|e| Error::io(src_dir.join(CONFIG_LEGACY), e))?;
        }
    } else {
        put(
            err,
            format!(
                "  {error}: Cannot recognize source: {source}\n  Expected: GitHub URL, .tar.gz file, or directory path.\n"
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    put(
        out,
        format!("  {} {label}\n\n", style.dim("Source:")).as_bytes(),
    )?;

    let Some(src_root) = find_ai_src(tmp) else {
        put(
            err,
            format!(
                "  {error}: No .ai/src/ (or .ai/) directory found in source.\n  The source must contain a structure created by {}.\n",
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let src_project_root = if src_root.ends_with("src") {
        src_root
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    } else {
        src_root.parent().map(Path::to_path_buf)
    }
    .unwrap_or_else(|| tmp.to_path_buf());
    let imported_config = [CONFIG, CONFIG_LEGACY]
        .iter()
        .map(|rel| src_project_root.join(rel))
        .find(|path| path.is_file());

    let local = resolve_sources(root);
    let dest_base_rel = if local.base.is_empty() {
        ".ai/src".to_string()
    } else {
        local.base.clone()
    };
    let dest_base = Path::new(root).join(&dest_base_rel);

    let mut targets: Vec<&'static str> = vec!["AGENTS.md"];
    targets.extend(DIR_TARGETS);
    if !only.is_empty() {
        targets = filter_targets(&targets, &only);
    }

    let mut changes = Vec::new();
    let mut counts = Counts::default();
    for target in &targets {
        let src_path = src_root.join(target);
        let dest_path = dest_base.join(target);
        if src_path.is_file() {
            diff_file(&src_path, &dest_path, target, &mut changes, &mut counts);
        } else if src_path.is_dir() {
            diff_dir(&src_path, &dest_path, target, &mut changes, &mut counts);
        }
    }
    let config_dest = Path::new(root).join(CONFIG);
    let mut config_action = "";
    if let Some(imported) = &imported_config {
        if config_dest.is_file() {
            if !same_bytes(imported, &config_dest) {
                config_action = "update";
                counts.updated += 1;
            }
        } else {
            config_action = "new";
            counts.new += 1;
        }
    }
    if changes.is_empty() && config_action.is_empty() {
        put(
            out,
            format!(
                "  {} Nothing to import.\n\n",
                style.green("Already up to date!")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }

    let mut text = format!("  {}\n", style.green("Changes:"));
    for change in &changes {
        match change {
            Change::New(name) => text.push_str(&format!(
                "    {} {name} {}\n",
                style.green("+"),
                style.dim("(new)")
            )),
            Change::Update(name) => text.push_str(&format!(
                "    {} {name} {}\n",
                style.yellow("~"),
                style.dim("(update)")
            )),
            Change::Dir(name) => text.push_str(&format!("    {} {name}\n", style.cyan("↳"))),
        }
    }
    if config_action == "new" {
        text.push_str(&format!(
            "    {} agent_sync.yaml {}\n",
            style.green("+"),
            style.dim("(new)")
        ));
    }
    if config_action == "update" {
        text.push_str(&format!(
            "    {} agent_sync.yaml {}\n",
            style.yellow("~"),
            style.dim("(update)")
        ));
    }
    text.push_str(&format!(
        "\n  {} {} new, {} updated, {} unchanged\n\n",
        style.dim("Summary:"),
        counts.new,
        counts.updated,
        counts.skipped
    ));
    if dry_run {
        text.push_str(&format!(
            "  {} — no files written.\n\n",
            style.yellow("Dry run")
        ));
        put(out, text.as_bytes())?;
        return Ok(0);
    }
    put(out, text.as_bytes())?;

    if !force && counts.updated > 0 && env.interactive {
        put(out, b"  Proceed? [Y/n] ")?;
        out.flush().map_err(|e| Error::io("<stdout>", e))?;
        let answer = (env.read_line)();
        if answer.starts_with(['N', 'n']) {
            put(out, b"  Cancelled.\n\n")?;
            return Ok(0);
        }
    }

    std::fs::create_dir_all(&dest_base).map_err(|e| Error::io(&dest_base, e))?;
    for target in &targets {
        let src_path = src_root.join(target);
        let dest_path = dest_base.join(target);
        if src_path.is_file() {
            copy_file(&src_path, &dest_path)?;
        } else if src_path.is_dir() {
            std::fs::create_dir_all(&dest_path).map_err(|e| Error::io(&dest_path, e))?;
            let mut files = Vec::new();
            files_below(&src_path, &mut files);
            for file in files {
                let rel = file.strip_prefix(&src_path).unwrap_or(&file);
                let dest_file = dest_path.join(rel);
                if let Some(parent) = dest_file.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }
                copy_file(&file, &dest_file)?;
            }
        }
    }
    if !config_action.is_empty()
        && let Some(imported) = &imported_config
    {
        if let Some(parent) = config_dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        copy_file(imported, &config_dest)?;
    }
    put(
        out,
        format!(
            "  {} {} new, {} updated files.\n\n  Next steps:\n    1. Review imported files in {}\n    2. Run {} to distribute to all tools\n\n",
            style.green("Imported!"),
            counts.new,
            counts.updated,
            style.cyan(&dest_base_rel),
            style.cyan("agentsync sync")
        )
        .as_bytes(),
    )
    .map(|()| 0)
}
```

In `src/main.rs`, before the `add` block:

```rust
    if let Some(command @ ("export" | "import")) = args.first().and_then(|a| a.to_str()) {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let logical_cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
        let env_root = var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty());
        let root = paths::logical_root(env_root.as_deref(), &cwd, var("PWD").as_deref());
        let style = Style::for_stdout();
        if command == "export" {
            return cli::bundle::export(
                &rest,
                &root,
                &style,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );
        }
        let mut read_line = || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line.trim_end_matches(['\n', '\r']).to_string()
        };
        let mut env = cli::bundle::Env {
            cwd: logical_cwd,
            interactive: std::io::stdin().is_terminal(),
            read_line: &mut read_line,
        };
        return cli::bundle::import(
            &rest,
            &root,
            &style,
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `export import`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor add export import "
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
printf 'bundle native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/bundle.bats | grep -c '^not ok')"
bats --tap -f 'parity: export|parity: import' tests/native_parity.bats
```

Expected: `277 passed`, `0`, `11`, `1`; `0`; `ok 1` and `ok 2`.

- [x] **Step 5: Prove the fixture bites, run the reference, lint, commit**

Change `Already up to date!` to `Already up to date` in `src/cli/bundle.rs`, rebuild, rerun `bats --tap -f 'parity: import' tests/native_parity.bats`: `not ok 1` with that line in the diff; revert and rebuild.

Recreate the harnesses when the session scratchpad no longer holds `phase4l/`. The reference takes `<engine 0|1|2> <repo root> <out file>` (mode 2 calls the debug binary directly), puts `fake_curl.sh` on `PATH` as `curl`, masks the project and work directories and the archive sizes, and prints each archive's sorted listing and each import's tree with short hashes; the probe takes `<repo root> <out dir>` and imports into one copy per engine.

`phase4l/fake_curl.sh`:

```bash
#!/usr/bin/env bash
# A curl stand-in for the bundle reference: honours `-o <file>`, takes the URL
# as the last argument, and serves $FAKE_GITHUB_DIR/<owner>_<repo>-<branch>.tar.gz
# when it exists, else fails as `curl -f` does on a 404.
out=""; url=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
printf '%s\n' "$url" >> "$FAKE_GITHUB_DIR/requests.log"
name="${url##*/archive/refs/heads/}"
repo="${url#https://github.com/}"; repo="${repo%%/archive/*}"
file="$FAKE_GITHUB_DIR/${repo//\//_}-$name"
[[ -f "$file" ]] || exit 22
cp "$file" "$out"
```

`phase4l/bundle_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: bundle_reference.sh <engine 0|1|2> <repo root> <out file>
# Runs every `export` and `import` branch and prints status, masked output,
# archive listings, and the files each import left behind. GitHub downloads go
# through fake_curl.sh on PATH, which serves prepared archives. Mode 2 calls
# the debug binary directly, for a command the dispatcher does not delegate yet.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/work_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK/bin" "$WORK/github"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
export FAKE_GITHUB_DIR="$WORK/github"
cp "$S/fake_curl.sh" "$WORK/bin/curl"
chmod +x "$WORK/bin/curl"
export PATH="$WORK/bin:$PATH"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH
N=0
P=""
fresh() {
    N=$((N + 1))
    P="$WORK/p$N"
    mkdir -p "$P"
    (cd "$P" && git init --quiet && git config user.email t@t && git config user.name T)
    cd "$P" || exit 1
}
bash_init() { AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" init --no-detect --yes --no-sync "$@" >/dev/null 2>&1; }
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    local physw; physw=$(cd -P "$WORK" && pwd)
    text=${text//"$physw"/<work>}
    text=${text//"$WORK"/<work>}
    # A gzip stream's length moves with the entries' mtimes, so the size is masked.
    printf '%s\n' "$text" | sed -E 's/\(([0-9]+) (B|KB|MB)\)/(<n> \2)/'
}
engine() {
    if [[ "$MODE" == 2 ]]; then
        AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" "$@"
    else
        bash "$REPO/bin/agentsync.sh" "$@"
    fi
}
# The archive's entries, sorted, and the files below a directory with their bytes.
listing() {
    local f
    for f in "$@"; do
        if [[ -f "$P/$f" ]]; then
            printf -- '--- archive %s\n' "$f"
            tar -tzf "$P/$f" | LC_ALL=C sort
            printf -- '--- end %s\n' "$f"
        else
            printf -- '--- %s absent\n' "$f"
        fi
    done
}
tree() {
    local dir="$1"
    printf -- '--- tree %s\n' "$dir"
    if [[ -d "$P/$dir" ]]; then
        (cd "$P" && find "$dir" -type f | LC_ALL=C sort | while IFS= read -r f; do
            printf '%s %s\n' "$f" "$(shasum -a 256 "$f" | cut -c1-12)"
        done)
    else
        echo "(absent)"
    fi
    printf -- '--- end tree %s\n' "$dir"
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
    output=$(cd "$P" && engine "$@" 2>&1) || rc=$?
    report "$name :: agentsync $*" "$rc" "$output"
}
run_env() {
    local name="$1" assignment="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && export "$assignment" && engine "$@" 2>&1) || rc=$?
    report "$name :: $assignment agentsync $*" "$rc" "$output"
}
run_list() {
    local name="$1" files="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1) || rc=$?
    local extra; extra=$(listing $files)
    report "$name :: agentsync $*" "$rc" "$output"$'\n'"$extra"
}
run_tree() {
    local name="$1" dir="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1) || rc=$?
    local extra; extra=$(tree "$dir")
    report "$name :: agentsync $*" "$rc" "$output"$'\n'"$extra"
}
# A project with two servers of its own to bundle; also the GitHub archives.
seed_bundle() {
    bash_init
    printf '# Seed rule\n' > "$P/.ai/src/rules/seed.md"
    mkdir -p "$P/.ai/src/tools/claude" "$P/.ai/src/settings"
    printf '{"a": 1}\n' > "$P/.ai/src/tools/claude/settings.json"
    printf '{"legacy": true}\n' > "$P/.ai/src/settings/cursor.json"
    printf '{"mcpServers": {}}\n' > "$P/.ai/src/mcp.json"
}

# ── export ───────────────────────────────────────────────────────────────────
fresh
run "export-no-ai" export
run "export-help" export --help
run "export-help-short" export -h
run "export-unknown" export --bogus
run "export-output-missing" export -o
mkdir -p "$P/.ai/src"
run "export-nothing" export
fresh; seed_bundle
run "export-dry-run" export --dry-run
run_list "export-default" "agentsync-bundle.tar.gz" export
run_list "export-output" "bundles/custom.tgz" export --output bundles/custom.tgz
mkdir -p "$P/bundles"
run_list "export-output-dir-made" "bundles/custom.tgz" export -o bundles/custom.tgz
run "export-output-is-dir" export -o bundles
mkdir -p "$P/sub"
run_env "export-repo-root" AGENTSYNC_REPO_ROOT="$P" export --dry-run
rc=0; output=$(cd "$P/sub" && AGENTSYNC_REPO_ROOT="$P" engine export -o rel.tgz 2>&1) || rc=$?
report "export-relative-output-from-sub :: (in sub, AGENTSYNC_REPO_ROOT=<root>) agentsync export -o rel.tgz" "$rc" "$output"$'\n'"$(listing rel.tgz sub/rel.tgz)"
mv "$P/.ai/agent_sync.yaml" "$P/agent_sync.yaml"
run "export-legacy-config" export --dry-run
rm "$P/agent_sync.yaml"
run "export-no-config" export --dry-run
fresh; bash_init
mkdir -p "$P/custom/rules" "$P/.ai/src/settings"
printf '# Custom\n' > "$P/custom/rules/x.md"
printf 'source:\n  rules: custom/rules\n  skills: nowhere/skills\n' >> "$P/.ai/agent_sync.yaml"
run "export-source-overrides" export --dry-run
fresh
mkdir -p "$P/.ai/rules" "$P/.ai/commands"
printf '# Old layout\n' > "$P/.ai/AGENTS.md"
printf '# R\n' > "$P/.ai/rules/r.md"
run_list "export-legacy-layout" "agentsync-bundle.tar.gz" export

# ── import: arguments and sources ────────────────────────────────────────────
fresh; bash_init
run "import-help" import --help
run "import-no-source" import
run "import-unknown" import --bogus
run "import-extra" import a b
run "import-only-missing" import --only
run "import-branch-missing" import --branch
run "import-unrecognized" import nothing.txt
printf 'not an archive\n' > "$P/broken.tar.gz"
run "import-broken-archive" import broken.tar.gz
mkdir -p "$P/plain/docs"
printf 'x\n' > "$P/plain/docs/readme.md"
run "import-dir-without-ai" import plain
(cd "$P/plain" && tar -czf "$P/no-ai.tgz" docs)
run "import-archive-without-ai" import no-ai.tgz

# ── import: archive into a fresh project, then updates ───────────────────────
fresh; seed_bundle
(cd "$P" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" export -o bundle.tgz >/dev/null 2>&1)
SRC="$P"
fresh
cp "$SRC/bundle.tgz" "$P/bundle.tgz"
run "import-archive-dry-run" import bundle.tgz --dry-run
run_tree "import-archive" ".ai" import bundle.tgz
run "import-archive-again" import bundle.tgz
printf '# Seed rule, edited\n' > "$SRC/.ai/src/rules/seed.md"
printf '# New rule\n' > "$SRC/.ai/src/rules/added.md"
printf 'outputs: local\n' >> "$SRC/.ai/agent_sync.yaml"
(cd "$SRC" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" export -o bundle2.tgz >/dev/null 2>&1)
cp "$SRC/bundle2.tgz" "$P/bundle2.tgz"
run "import-update-dry-run" import bundle2.tgz --dry-run
run_tree "import-only-nothing" ".ai" import bundle2.tgz --only bogus
run_tree "import-update-only-skills" ".ai" import bundle2.tgz --only skills
run_tree "import-update-only-agents-md" ".ai" import bundle2.tgz --only " AGENTS , bogus "
run_tree "import-update" ".ai" import bundle2.tgz
run_tree "import-update-force" ".ai" import bundle2.tgz --force

# ── import: directories and layouts ──────────────────────────────────────────
fresh
mkdir -p "$P/other"
cp -R "$SRC/.ai" "$P/other/.ai"
run_tree "import-directory" ".ai" import other
run_tree "import-directory-trailing-slash" ".ai" import other/
mkdir -p "$P/legacy/.ai/rules"
printf '# Legacy\n' > "$P/legacy/.ai/AGENTS.md"
printf '# LR\n' > "$P/legacy/.ai/rules/lr.md"
printf 'outputs: committed\n' > "$P/legacy/agent_sync.yaml"
run_tree "import-legacy-layout-directory" ".ai" import legacy
fresh
mkdir -p "$P/.ai/rules"
printf '# Mine\n' > "$P/.ai/AGENTS.md"
cp "$SRC/bundle.tgz" "$P/bundle.tgz"
run_tree "import-into-legacy-layout" ".ai" import bundle.tgz

# ── import: GitHub through the curl stand-in ─────────────────────────────────
fresh
mkdir -p "$WORK/gh/repo-main/.ai/src/rules" "$WORK/gh/repo2-master/.ai/src"
printf '# From GitHub\n' > "$WORK/gh/repo-main/.ai/src/AGENTS.md"
printf '# GH rule\n' > "$WORK/gh/repo-main/.ai/src/rules/gh.md"
printf '# From master\n' > "$WORK/gh/repo2-master/.ai/src/AGENTS.md"
(cd "$WORK/gh" && tar -czf "$FAKE_GITHUB_DIR/user_repo-main.tar.gz" repo-main && tar -czf "$FAKE_GITHUB_DIR/user_repo2-master.tar.gz" repo2-master)
run_tree "import-github" ".ai" import https://github.com/user/repo
run "import-github-again-dot-git" import https://github.com/user/repo.git/
run_tree "import-github-master-fallback" ".ai" import https://github.com/user/repo2 --force
run "import-github-branch-missing" import https://github.com/user/repo3
run "import-github-tree-branch" import https://github.com/user/repo/tree/develop
run "import-github-explicit-branch" import https://github.com/user/repo -b feature/x
run "import-github-bad-url" import https://github.com/user
run "import-github-www" import https://www.github.com/user/repo --only rules --dry-run
{
    printf -- '--- curl requests\n'
    mask < "$FAKE_GITHUB_DIR/requests.log"
    printf -- '--- end curl requests\n'
} >> "$OUT"
```

`phase4l/tiny_probe.sh`:

```bash
#!/usr/bin/env bash
# Usage: tiny_probe.sh <repo> <out dir>: export --dry-run on a four-file project
# and a directory import of it into an empty one, both engines, stdout and
# stderr apart, each engine importing into its own copy. The unit tests
# assert these transcripts.
set -uo pipefail
REPO="$1"; OUT="$2"
if [[ -d "$OUT" ]]; then rm -r "$OUT"; fi
A="$OUT/a"
mkdir -p "$A/.ai/src/rules" "$A/.ai/src/skills/x" "$A/custom/cmds" "$OUT/b0" "$OUT/b2"
printf '# Agents\n' > "$A/.ai/src/AGENTS.md"
printf '# A rule\n' > "$A/.ai/src/rules/a.md"
printf '# Skill\n' > "$A/.ai/src/skills/x/SKILL.md"
printf '# Command\n' > "$A/custom/cmds/c.md"
printf 'source:\n  commands: custom/cmds\n' > "$A/.ai/agent_sync.yaml"
export AGENTSYNC_HOME="$REPO"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH
run_bash() { (cd "$1" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" "${@:2}"); }
run_rust() { (cd "$1" && AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" "${@:2}"); }
compare() {
    local name="$1"
    diff "$OUT/$name.bash.out" "$OUT/$name.rust.out" && echo "$name stdout same"
    diff "$OUT/$name.bash.err" "$OUT/$name.rust.err" && echo "$name stderr same"
    echo "--- $name bash stdout:"; cat "$OUT/$name.bash.out"
    echo "--- $name bash stderr:"; cat "$OUT/$name.bash.err"
}
run_bash "$A" export --dry-run > "$OUT/export.bash.out" 2> "$OUT/export.bash.err"; echo "export bash rc=$?"
run_rust "$A" export --dry-run > "$OUT/export.rust.out" 2> "$OUT/export.rust.err"; echo "export rust rc=$?"
compare export
run_bash "$OUT/b0" import ../a > "$OUT/import.bash.out" 2> "$OUT/import.bash.err"; echo "import bash rc=$?"
run_rust "$OUT/b2" import ../a > "$OUT/import.rust.out" 2> "$OUT/import.rust.err"; echo "import rust rc=$?"
compare import
diff <(cd "$OUT/b0" && find .ai -type f | LC_ALL=C sort) <(cd "$OUT/b2" && find .ai -type f | LC_ALL=C sort) && echo "import trees same"
```

```bash
bash phase4l/bundle_reference.sh 0 "$PWD" phase4l/ref_bash.out && bash phase4l/bundle_reference.sh 1 "$PWD" phase4l/ref_native.out
wc -l < phase4l/ref_native.out
diff phase4l/ref_bash.out phase4l/ref_native.out | grep -c '^[<>]'
bash phase4l/tiny_probe.sh "$PWD" phase4l/tiny_probe | grep -c same
```

Expected: `1150` lines; `0` differing lines; `4`, stdout and stderr identical for the dry-run export and the directory import.

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/bundle.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "feat(native): port export and import"
```

---

### Task 6: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
50. `import` strips a `.git` suffix before a trailing `/`, so
    `https://github.com/user/repo.git/` downloads the repository `repo.git`.
51. A directory `import` copies the source project's `.ai/` alone, so a
    `source:` override pointing elsewhere in that project is not carried.
```

Append to "Accepted deviations":

```markdown
- Phase 4l: `export`'s archive bytes are not compared; two `tar -czf` runs on
  the same files differ in the gzip header's time, so the parity fixtures
  compare the report and the archive's listing.
- Phase 4l: `import` extracts into a scratch directory under the system temp
  dir, removed when the command ends, where Bash used the run directory.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/export.sh` row to `→ src/cli/bundle.rs       Phase 4l, ported; tar through the executable`, and the `lib/helpers/import.sh` row to `→ src/cli/bundle.rs       Phase 4l, ported; curl and tar through the executables`. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase4i/native_suite.sh "$PWD" both phase4l/suite_both.out && tail -1 phase4l/suite_both.out
```

Expected: `277 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 50 bats files, each run one at a time under both engines.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-4l-bundle.md
git commit -m "docs(native): map the phase 4l modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the two parity fixtures pass, and a `## Completion receipt` records the fresh verification. The last plan of Phase 4 covers `generate`, `shell-init`, and `setup-hooks`.

## Run log

### 2026-09-16 — Phase 4l planned
- Commits: this plan.
- Verified: every branch of `cmd_export` and `cmd_import` was captured with `bundle_reference.sh` (48 scenarios, archive listings and trees included) through a `curl` stand-in. The reference turned up four Bash bugs, each covered by a test in the new `tests/bundle.bats` that fails on the committed engine: `import` dies on `_BUNDLE_CONFIG` because the dispatcher never loads `export.sh`; a source without `.ai` exits silently; a relative `--output` is sized from the working directory; an empty selection dies on an unbound array under Bash 3.2. The Rust in Task 5 was drafted in the tree: `cargo test` 277/0/11/1 with the 4k port, fmt and clippy clean; the debug binary, called directly, gave a 1150-line transcript identical to the fixed Bash, and identical stdout and stderr on the probe. Baseline `cargo test` 271/0/11/1; `native_parity.bats` 60 cases green in Bash.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
