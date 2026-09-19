# Rust Migration Phase 6: Retire Bash

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the binary the only engine in the repository: the bats suite drives `target/release/agentsync` directly on every platform, including Windows unsharded by need rather than by Bash's limits; the Bash-unit bats files become Rust unit tests; `bin/agentsync.sh`, `lib/sync.sh`, `lib/check.sh`, `lib/setup_hooks.sh`, and `lib/helpers/` are deleted with `_native_try`, `AGENTSYNC_NATIVE`, and the parity harness; ShellCheck covers `install.sh` and `lib/templates/guard/claude.sh` alone. Phase 7 then retires bats itself.

**Architecture:** The seam moves from the dispatcher to the binary: `tests/test_helper.bash` points `AGENTSYNC_BIN` at the release build and `run_agentsync` executes it, and every `bash "$AGENTSYNC_BIN"` in the CLI-level files becomes `"$AGENTSYNC_BIN"`, so the 43 conformance files change in one mechanical sweep. The three fixtures that copied `bin/` and `lib/` (`release.bats`, `install.bats`, and `install.sh`'s source path) stop depending on Bash: `release` recognises a checkout by `Cargo.toml` and `VERSION`, `release.bats` seeds a checkout with those two files, and `install.bats` keeps its source-install cases against a stub `bin/agentsync.sh` in the fixture origin because pins older than 0.37.0 still clone the repository at a tag that has the real one. Windows comes first: the binary run directly from Git Bash receives MSYS paths in `AGENTSYNC_REPO_ROOT`, `TMPDIR`, and `PWD`, and the runner's console reads as a terminal, both found by run 35424805832; the binary translates a `/`-rooted path through `cygpath -w` when `MSYSTEM` is set and treats stdin as a terminal only when it is a real console. The Bash-unit bats files (`files`, `paths`, `backup`, `tmp`, `gitignore`, `update_snapshot`, plus `changelog_render` and `update`, which the spec's list omits but which also source `lib/`) are retired against a case-by-case table: each `@test` maps to an existing Rust test or gains one; `update.bats` tests the git reconcile Bash's `update` did, which the binary does not, and is retired without a counterpart. Only then are the Bash files deleted, and the docs, rules, skills, and commands under `.ai/src` that describe the strangler are rewritten for one engine.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new crate. bats-core for the conformance suite until Phase 7. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 6". Previous plan: `docs/plans/2026-09-18-rust-migration-phase-5e-cutover.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `lib/templates/`, `lib/config.yaml`, and `lib/prompts/` stay: the binary embeds them. `lib/templates/guard/claude.sh` stays POSIX `sh`.
- The CLI-level bats files stay the conformance suite; their assertions do not change in this phase. A behaviour difference that surfaces once the binary is called directly is a bug in the binary or the fixture, never a reason to edit an assertion.
- Every Bash-unit `@test` retired in Task 3 is accounted for in the plan's table: the Rust test that covers it (existing or added, named after the bats case), or `retired` with the reason. `cargo test` never loses a test.
- The Bash files are deleted in Task 4 only after Task 1 (the suite on the binary, Linux and macOS green), Task 2 (Windows green), and Task 3 (the unit ports) are committed: the reference stays until nothing needs it.
- Windows is a required check from Task 2 on. The shard matrix stays for the binary: it exists because Git Bash cannot run `bats --jobs` and a serial run exceeds 90 minutes (run 35424805832), not because of Bash; the spec's "remove the Windows shard matrix" is read as "the Windows gate runs the binary", recorded as a deviation.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state; `cygpath` is spawned only when `MSYSTEM` is set.
- ShellCheck runs on `install.sh` and `lib/templates/guard/claude.sh` after Task 4; the `lint` job lists those two.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time locally; CI runs `--jobs 4` on Linux and macOS.

## Decisions for the review

1. **The Windows shard matrix stays, pointed at the binary.** Serial bats under Git Bash took over 90 minutes even for the file subset that ran (run 35424805832); sharding twelve ways is what puts Windows at 5 minutes. The spec expected the binary to make the serial run fast; the cost is in bats and Git Bash process spawning, not in the engine. Alternative: serial with `continue-on-error`; rejected, Windows would never gate.
2. **Windows first (Task 2), deletion last (Task 4).** The Bash shards are the only Windows gate today; deleting Bash before the binary's Windows run is green leaves Windows unguarded. Task 2 is the one task in this plan that cannot be verified on the development host: it iterates with CI, one commit per finding, each pushed by the maintainer, and its expected values are written when it closes.
3. **`release` recognises a checkout by `Cargo.toml` and `VERSION`.** `src/cli/release.rs` still requires `bin/agentsync.sh`; after Task 4 that file is gone and `release` would refuse the repository. `Cargo.toml` with `name = "agentsync"` is the marker `release` already reads for the crate bump. Accepted deviation from the Bash check, recorded in Task 1.
4. **`install.sh` keeps the source path; `install.bats` keeps its cases against a stub.** A pin older than 0.37.0 still clones the repository and checks out a tag that has the real `bin/agentsync.sh`, so the path stays; the fixture origin cannot copy `bin/` and `lib/` from a checkout that no longer has them, so it commits a two-line `bin/agentsync.sh` that prints `agentsync v$VERSION`, enough for the link and version assertions. The `update <version>` cases that ran Bash's git-based `update` and the switch to the binary are retired: that code lives only in the 0.37.0 tag now, where it did its job (the maintainer's install switched on 2026-09-19).
5. **`update.bats` is retired without a Rust counterpart; `changelog_render.bats` maps onto `src/changelog.rs`.** The spec names six Bash-unit files; `changelog_render` and `update` also source `lib/` and would break in Task 4. The changelog cases are already asserted in `changelog::tests` (rendering without markers, wrapping, width clamp, section selection); the git reconcile has no binary equivalent.
6. **The `test-unix` Bash jobs go; the `native` job becomes the suite.** One engine, one run per OS; the job keeps its name `Native engine` until Phase 7 renames the workflow.
7. **`scripts/perf/bench.sh` and `docs/perf/` stay** as the record of the Bash baseline the spec cites; `bench.sh` gains one line noting it benchmarked the retired engine. Alternative: delete; rejected, the spec's "Why" table references it.
8. **Task order:** the suite on the binary (Task 1), Windows (Task 2), the unit ports (Task 3), the deletion (Task 4), the docs (Task 5). **Recommended:** as listed.

## Module closure

```text
tests/test_helper.bash           5, 14, 53-55, 110   AGENTSYNC_BIN, AGENTSYNC_NATIVE, run_agentsync, seed_project
tests/*.bats (24 files)                              `bash "$AGENTSYNC_BIN"` call sites (grep -l 'bash "\$AGENTSYNC_BIN"')
tests/release.bats               26-27, 50, 77-91    the seed copies bin/ and lib/; `bash bin/agentsync.sh release`
tests/install.bats               38, 226-262         the origin fixture copies bin/ and lib/; the Bash update cases
tests/native_dispatch.bats, tests/native_parity.bats deleted in Task 1
tests/files.bats 36, paths.bats 27, backup.bats 18, tmp.bats 15, gitignore.bats 7, update_snapshot.bats 20, changelog_render.bats 13, update.bats 6   Task 3
src/cli/release.rs               the checkout check (`bin/agentsync.sh`)
src/paths.rs                     logical_root (MSYS translation, Task 2)
src/prompts.rs                   is_tty (the runner console, Task 2)
src/main.rs                      project_root, notice_root, the engine-version guard (AGENTSYNC_ENGINE_VERSION goes with the dispatcher)
bin/agentsync.sh, lib/sync.sh, lib/check.sh, lib/setup_hooks.sh, lib/helpers/*.sh (45)   deleted in Task 4
.github/workflows/ci.yaml        lint, test-unix, test-windows, native
install.sh                       the source path stays; comments name the tag boundary
README.md, .ai/src/AGENTS.md, .ai/src/rules/{core,architecture,native-engine}.md, .ai/src/skills/native-port/**, .ai/src/commands/native-*.md, .ai/src/commands/{fix-issue,release}.md, .ai/src/skills/{fix-issue,release}/SKILL.md   Task 5
docs/specs/2026-09-12-rust-migration-design.md       the dispatcher section, Phase 6, the status line
```

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
ls tests/*.bats | wc -l
grep -l 'bash "\$AGENTSYNC_BIN"' tests/*.bats | wc -l
grep -c 'bin/agentsync.sh' src/cli/release.rs
ls lib/helpers/*.sh | wc -l
```

Expected: `5613d4c fix(prompts): read a terminal answer a byte at a time like read -r` or later; `326 passed`, `0 passed`, `11 passed`, `1 passed`; `51`; `14` (the plan's 24 counted the files that source `lib/` too); `2`; `45`.

---

### Task 1: The suite drives the binary

**Files:**
- Modify: `tests/test_helper.bash`, the 24 bats files with `bash "$AGENTSYNC_BIN"`, `tests/release.bats`, `tests/install.bats`, `src/cli/release.rs`, `.github/workflows/ci.yaml`, `docs/specs/2026-09-12-rust-migration-design.md` (one deviation line)
- Delete: `tests/native_dispatch.bats`, `tests/native_parity.bats`

**Interfaces:**

```bash
# tests/test_helper.bash
AGENTSYNC_BIN="$REPO_ROOT/target/release/agentsync"   # or agentsync.exe when that exists
run_agentsync() { "$AGENTSYNC_BIN" "$@"; }             # AGENTSYNC_HOME no longer set
```

```rust
// src/cli/release.rs: a checkout is a directory with VERSION and Cargo.toml naming the crate
```

- [x] **Step 1: The helper and the call sites**

In `tests/test_helper.bash` replace the `AGENTSYNC_BIN` line with a lookup of `target/release/agentsync` then `target/release/agentsync.exe`, export it (the `bash -c '…'` call sites expand it in a child shell), delete the `AGENTSYNC_NATIVE` export and its comment, add `AGENTSYNC_HOME` to the `unset` line (without it `release` falls back to the developer's `~/.agentsync` when the working directory is not a checkout), and make `run_agentsync` and `seed_project` execute `"$AGENTSYNC_BIN"`. Then, in every bats file, replace the four call forms with a `sed` script file (a write-then-`mv`, one file at a time): `AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN"` and `bash "$AGENTSYNC_BIN"`, their single-quoted twins inside `bash -c "…"`, and `SHELL=/bin/bash "$AGENTSYNC_BIN"` kept intact; the `script -c` line of `init.bats` and the PATH shim of `hooks.bats` by hand. Remove `tests/native_dispatch.bats` and `tests/native_parity.bats`, and the two `rollback_preflight.bats` cases guarded by `AGENTSYNC_NATIVE != 1` (they shimmed `sha256sum` for Bash's seal; the binary hashes in-process, Phase 3b deviation).

Run: `grep -rn 'bash "\$AGENTSYNC_BIN"\|bash '"'"'\$AGENTSYNC_BIN'"'"'' tests/ | wc -l; grep -rn 'AGENTSYNC_NATIVE\b' tests/ | wc -l; ls tests/*.bats | wc -l; grep -c '^@test' tests/rollback_preflight.bats`
Expected: `0`; `0`; `49`; `28`.

- [x] **Step 2: `release` recognises a checkout without the dispatcher**

In `src/cli/release.rs`, replace the `bin/agentsync.sh` condition with `Cargo.toml` (the file `crate_version` already reads), and update the unit test that names `bin/agentsync.sh`: a checkout without `Cargo.toml` is now refused as `Must be run from the AgentSync repository.`, so the `refusals_leave_the_checkout_untouched_like_release_sh` assertion and the bats case `release fails without Cargo.toml` assert that message. In `tests/release.bats`, seed the checkout with `VERSION`, `CHANGELOG.md`, `Cargo.toml`, and `Cargo.lock` only, drop the `AGENTSYNC_NATIVE_BIN` lookup in `setup`, and run `"$AGENTSYNC_BIN" release …` in it. Record under "Accepted deviations": `Phase 6: release recognises a checkout by VERSION and Cargo.toml, and a directory without Cargo.toml is refused as Must be run from the AgentSync repository.; Bash looked for bin/agentsync.sh, which Phase 6 deletes, and reported the missing manifest as a missing crate version.`

Run: `cargo test release 2>&1 | grep 'test result' | head -1; bats --tap tests/release.bats | grep -c '^ok'`
Expected: every `release` test passing; `18`.

- [x] **Step 3: `install.bats` without `bin/` and `lib/` to copy**

In `_build_origin_fixture`, replace `cp -R "$REPO_ROOT/bin" "$REPO_ROOT/lib" .` with a committed stub `bin/agentsync.sh` (two lines: the shebang and `echo "agentsync v$(cat "$(dirname "$0")/../VERSION")"`) plus a `lib/helpers/.keep` (the installer checks `lib/` exists for a source install). Retire the five `update` cases at the end of the file (they ran Bash's `update` and its switch to the binary); keep the nine installer cases.

Run: `grep -c '^@test' tests/install.bats; bats --tap tests/install.bats | grep -c '^ok'`
Expected: `9`; `9`.

- [x] **Step 4: The suite on Linux and macOS**

```bash
cargo build --release 2>&1 | tail -1
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: `Finished`; only `suite-done` (every file 0 failures on the development host; the Linux run is the pushed CI).

- [x] **Step 5: CI**

In `.github/workflows/ci.yaml`: delete the `test-unix` job; in `test-windows` (renamed `Native engine (windows, shard n/12)`) build the binary first (`dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `cargo build --release`) and keep the shard run with `--print-output-on-failure`; in `native` drop the Windows leg and the advisory step (the shards are the Windows gate), keep `cargo fmt`, `clippy`, `cargo test`, `cargo build --release`, and the `bats --jobs 4 --tap --print-output-on-failure tests/` run on Linux and macOS without `AGENTSYNC_NATIVE`; the `lint` job keeps its full list until Task 4. Validate with `ruby -ryaml -e 'YAML.safe_load(File.read(".github/workflows/ci.yaml"), aliases: true)'`.

- [x] **Step 6: Commit**

```bash
git add tests/ src/cli/release.rs .github/workflows/ci.yaml docs/specs/2026-09-12-rust-migration-design.md
git commit -m "test: drive the bats suite through the binary"
```

The maintainer pushes; Linux and macOS must be green before Task 2 starts. Windows is expected red here.

---

### Task 2: Windows runs the suite against the binary

**Files:**
- Modify: `src/paths.rs`, `src/prompts.rs`, `src/main.rs`, possibly `tests/test_helper.bash`
- Test: the Windows shard jobs

**Interfaces:**

```rust
// src/paths.rs
/// A `/`-rooted path handed over by Git Bash, translated through `cygpath -w`
/// when `MSYSTEM` is set; any other path unchanged.
pub fn from_msys(path: &str, msystem: Option<&str>) -> String;
// src/prompts.rs
pub fn is_tty() -> bool;   // on Windows: false unless stdin and stdout are a real console
```

- [ ] **Step 1: Read the first Windows shard logs**

`gh api repos/yelmuratoff/agent_sync/actions/jobs/<id>/logs` for each red shard of the Task 1 push; list every distinct failure message with the test that produced it. The two known ones: `Error: Directory not found: .` (a POSIX `AGENTSYNC_REPO_ROOT`, `TMPDIR`, or `PWD` reaching the binary) and a hang after `dedupe requires TTY without --yes` (a prompt read from `CONIN$`).

- [ ] **Step 2: Translate MSYS paths**

Add `paths::from_msys` and apply it in `main.rs` wherever an environment path is read (`AGENTSYNC_REPO_ROOT`, `AGENTSYNC_CONFIG_PATH`, `PWD`, `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`, `TMPDIR` if read) and to positional path arguments (`init <dir>`, `adopt <file>`, `import <source>`). Unit-test the pure part: with `msystem` `None` the path is unchanged; with `Some("MINGW64")` a Windows path is unchanged and a `/`-rooted one goes through the translator (inject the translator as a closure so the test needs no `cygpath`).

- [ ] **Step 3: The runner console**

On Windows make `prompts::is_tty` require a real console: `std::io::stdin().is_terminal()` is true for the runner's console even under bats; check `MSYSTEM` is unset or that `CONIN$` opens and reports a console mode. Reproduce with the first hang's test.

- [ ] **Step 4: Iterate with CI**

Commit each fix as `fix(windows): …`, the maintainer pushes, read the shards. Close the task when all twelve shards are green twice in a row. Record the final expected values here: the number of iterations, each root cause, and the commit that fixed it.

- [ ] **Step 5: Commit the closing note**

```bash
git add docs/plans/2026-09-19-rust-migration-phase-6-retire-bash.md
git commit -m "docs(native): record the Windows fixes of phase 6"
```

---

### Task 3: The Bash-unit files become Rust tests

**Files:**
- Modify: `src/filters.rs`, `src/file_ops.rs`, `src/rules.rs`, `src/convert.rs`, `src/paths.rs`, `src/backup.rs`, `src/staging.rs`, `src/gitignore.rs`, `src/snapshot.rs`, `src/changelog.rs` (tests only)
- Delete: `tests/files.bats`, `tests/paths.bats`, `tests/backup.bats`, `tests/tmp.bats`, `tests/gitignore.bats`, `tests/update_snapshot.bats`, `tests/changelog_render.bats`, `tests/update.bats`

- [ ] **Step 1: The table**

For each of the 142 cases (36 + 27 + 18 + 15 + 7 + 20 + 13 + 6), write one row `file | bats case | Rust test | status` into a `### Bash-unit cases` section of this plan, where status is `existing` (the named Rust test asserts the same value), `added` (a new test named after the case, asserting the value the Bash helper produced, confirmed by running the helper before deletion), or `retired` (with the reason: `tmp.sh`'s run-directory lifecycle and `update.sh`'s git reconcile have no counterpart in the binary; `paths.sh`'s `REPLY` wrappers and memoisation are Bash mechanics). Expected: no row without a status; `retired` rows only for the three groups named.

- [ ] **Step 2: Add the missing tests**

One commit per Rust module. Every added test asserts a value captured by running the Bash helper (`bash -c 'source lib/helpers/x.sh; …'`) before Task 4 deletes it.

Run: `cargo test 2>&1 | grep 'test result' | head -1`
Expected: `326 + <added>` passed, the number written into the table's footer.

- [ ] **Step 3: Delete the eight files**

```bash
git rm tests/files.bats tests/paths.bats tests/backup.bats tests/tmp.bats tests/gitignore.bats tests/update_snapshot.bats tests/changelog_render.bats tests/update.bats
ls tests/*.bats | wc -l
git commit -m "test: retire the Bash-unit bats files"
```

Expected: `41`.

---

### Task 4: Delete the Bash engine

**Files:**
- Delete: `bin/agentsync.sh`, `lib/sync.sh`, `lib/check.sh`, `lib/setup_hooks.sh`, `lib/helpers/` (45 files)
- Modify: `.github/workflows/ci.yaml` (the `lint` job), `install.sh` (comments), `scripts/perf/bench.sh` (one note), `.gitignore` (`.last_update_check`, `.snapshot/` no longer produced), `src/main.rs` (the `AGENTSYNC_ENGINE_VERSION` guard and `Error::StaleBinary` go with the dispatcher)

- [ ] **Step 1: Delete and re-point**

```bash
git rm -r bin/agentsync.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers
ls lib/
```

Expected: `config.yaml  prompts  templates`.

Then remove `guard_engine_version` and `Error::StaleBinary` (and the `tests/cli.rs` case that exercises them, if any), set the `lint` job to `shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh`, and drop the `.ai/src` references Task 5 does not rewrite.

- [ ] **Step 2: Verify**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings; cargo test 2>&1 | grep 'test result' | head -4
cargo build --release 2>&1 | tail -1
shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh; echo "shellcheck=$?"
grep -rn 'agentsync\.sh\|lib/helpers\|AGENTSYNC_NATIVE\|_native_try' --include='*.bats' --include='*.bash' --include='*.rs' --include='*.yaml' --include='*.sh' . | grep -v '^./target\|^./docs/plans\|^./docs/specs\|install.sh' | wc -l
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: fmt and clippy exit 0, the test counts of Task 3; `Finished`; `shellcheck=0`; `0`; only `suite-done`.

- [ ] **Step 3: Commit**

```bash
git add -A bin lib .github/workflows/ci.yaml install.sh scripts/perf/bench.sh .gitignore src tests
git commit -m "feat(native): retire the Bash engine"
```

---

### Task 5: Docs, rules, skills, and the module map

**Files:**
- Modify: `README.md` (the Development and Native engine sections, the "Windows (Git Bash)" mentions), `.ai/src/AGENTS.md` (Tech Stack, Boundaries, Verify), `.ai/src/rules/core.md` and `.ai/src/rules/architecture.md` (the Bash module map becomes the Rust one), `.ai/src/rules/native-engine.md` (no dispatcher), `.ai/src/skills/native-port/SKILL.md` and its references (the port is over: the skill becomes the record of the Bash semantics the binary mirrors, or is retired), `.ai/src/commands/native-*.md` (`native-next` gains the Phase 7 rule set; `native-parity` and `native-port` are retired), `.ai/src/commands/{fix-issue,release}.md`, `.ai/src/skills/{fix-issue,release}/SKILL.md`, `docs/specs/2026-09-12-rust-migration-design.md` (dispatcher section, Phase 6 receipt pointer, status line), `.ai/.sync-manifest` (regenerated by `agentsync sync --force` with the binary)

- [ ] **Step 1: Rewrite and regenerate**

Every mention of `bin/agentsync.sh`, `lib/helpers`, `AGENTSYNC_NATIVE`, `_native_try`, "pure Bash", and "Git Bash" outside `docs/plans` and the spec's history is rewritten or removed. Run `agentsync sync --force` and `agentsync check`.

Run: `grep -rn 'AGENTSYNC_NATIVE\|_native_try\|lib/helpers\|bin/agentsync\.sh' README.md .ai/src | wc -l; agentsync check > /dev/null; echo "check=$?"`
Expected: `0`; `check=0`.

- [ ] **Step 2: Commit**

```bash
git add README.md .ai docs/specs/2026-09-12-rust-migration-design.md
git commit -m "docs(native): describe the one-engine repository"
```

---

## Completion

The plan is closed when every box is ticked, the suite is green on Linux, macOS, and all twelve Windows shards against the binary, `cargo test` carries every Bash-unit case the table maps, `bin/agentsync.sh` and `lib/helpers/` no longer exist, ShellCheck covers the two remaining scripts, and a `## Completion receipt` records the fresh verification. The Windows task's expected values are written when it closes, since only CI can produce them. Phase 7 (retire bats) follows.

## Run log

### 2026-09-19 — Phase 6 planned
- Commits: this plan, on `feat/native-engine-phase-6` created from `main` at `5613d4c` (the Phase 5 branch is merged and stays as history).
- Verified: the closure was read from the tree: 24 bats files call `bash "$AGENTSYNC_BIN"`, `release.bats` and `install.bats` copy `bin/` and `lib/` into their fixtures, `src/cli/release.rs` names `bin/agentsync.sh` twice, `lib/helpers/` holds 45 files, the Bash-unit files count 142 cases including `changelog_render` and `update`, which the spec's list omits. The Windows facts come from run 35424805832 (2026-09-19): every command through the binary failed with `Directory not found: .`, and the run hung after `dedupe requires TTY without --yes` until the 90-minute limit. No code was drafted: Task 1 is mechanical, Task 2 needs CI, Task 3 is a table the task produces, Task 4 is deletion.
- Plan amended: none.
- Next: Task 0 Step 1, after the review; the maintainer pushes after each of Tasks 1, 2 (per fix), 3, 4, 5, since Linux and Windows are checked only in CI.
- Blocker: none.

### 2026-09-19 — Tasks 0 and 1 done
- Commits: this commit, test: drive the bats suite through the binary. The maintainer asked to continue; the eight review decisions taken as recommended.
- Verified: baseline at `1dac2c1`: 326/0/11/1, 51 files, 14 files with `bash "$AGENTSYNC_BIN"` (the plan said 24; amended), `release.rs` naming the dispatcher twice, 45 helpers. Task 1: no dispatcher call form left, no `AGENTSYNC_NATIVE` in `tests/`, 49 files, `rollback_preflight` 28 cases; `cargo test release` 10/10 with the new refusal; `release.bats` 18/18; `install.bats` 9/9 against the stub origin; every bats file against `target/release/agentsync` one at a time, outside the sandbox for the pty and `diff` cases: `TOTAL 0` over 49 files (the first sweep left 38 failures in eight files, all from two `sed` misses: the single-quoted call form inside `bash -c` and `SHELL=/bin/bash`, plus the `init` pty line, the `hooks` PATH shim, and two Bash-only seal cases); fmt, clippy, `cargo test` 326/0/11/1, ShellCheck on the helper exit 0; the workflow parsed with three jobs. A near miss found and closed: without `AGENTSYNC_HOME` in the helper's `unset`, `release fails without Cargo.toml` fell back to the developer's `~/.agentsync` (its dirty tree made `release` refuse; nothing was changed there).
- Plan amended: Task 0 count 14; Task 1 Step 1 names the four call forms, the `sed` script file, the `unset`, the export, and the two retired `rollback_preflight` cases; Step 2 names the new refusal message and its two assertions; Step 3 retires five cases; Step 5 keeps the full lint list until Task 4.
- Next: the maintainer pushes `feat/native-engine-phase-6`; Linux and macOS must be green, Windows shards are expected red until Task 2. Then Task 2 Step 1: read the shard logs.
- Blocker: none.
