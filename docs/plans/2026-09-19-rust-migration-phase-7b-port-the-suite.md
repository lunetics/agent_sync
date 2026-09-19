# Rust Migration Phase 7b: Port the Rest of the Suite

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port the remaining 38 bats files (701 cases) to Rust integration tests on the harness plan 7a built, one commit per bats file, then delete `tests/test_helper.bash`, drop bats and GNU parallel from CI, and remove the Windows shard matrix. Exit: `cargo test` is the whole suite and no `.bats` file remains. That closes Phase 7 and the migration.

**Architecture:** `tests/common/mod.rs` (plan 7a) is the shared harness and does not change: a task that needs a fixture only its own files use defines it in its own test crate, because each `tests/<name>.rs` compiles separately and duplication there costs nothing. A ported test carries its bats name with spaces and punctuation replaced by `_`, and asserts on the stream the binary actually writes to — `bats run` merged stdout and stderr into `$output`, so a port names `stdout` or `stderr` explicitly. What bats skipped, the port skips the same way and says why: `#[cfg(unix)]` for a case that needs a shell stand-in on `PATH`, a FIFO, a POSIX hook, or `chmod` semantics; an early return where the bats case skipped at runtime (as root, or without a tool). A case the Rust suite already asserts is retired, with the covering test named in the commit body.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new crate. bats-core until the last task. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 7". Previous plan: `docs/plans/2026-09-19-rust-migration-phase-7a-harness-cli-list-check.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `lib/templates/`, `lib/config.yaml`, and `lib/prompts/` stay: the binary embeds them. `lib/templates/guard/claude.sh` stays POSIX `sh`.
- A ported test asserts what its bats case asserted, on the stream the binary writes it to. A behaviour difference a port exposes is a bug in the binary or the fixture, never a reason to weaken the assertion; it stops the task and is reported.
- Every bats case of a ported file is accounted for: a Rust test named after it, or retired with the covering Rust test named in the commit body. `cargo test` never loses a test.
- One bats file per commit, `test(native): port <file>.bats`; the commit deletes the bats file.
- `tests/common/mod.rs` is shared: a task extends it only when two tasks need the same helper, and says so in its commit body.
- Windows is a required check: `cargo test` runs in the first Windows shard until the last task removes the matrix. A test that cannot run there is `#[cfg(unix)]` with a comment naming the quirk.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. No test spawns a shell to do what the binary does; a shell stand-in for a program the binary spawns (`curl`, `pbcopy`, an editor) is allowed on Unix only, as in bats.
- ShellCheck stays on `install.sh` and `lib/templates/guard/claude.sh`.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Every verification block runs `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, and, while `.bats` files remain, the per-file bats sweep.

## Decisions for the review

1. **Batches, not one task per file.** 38 files at one plan task each would be 38 review units for a mechanical translation. A task is a batch of files that share a fixture; it still makes one commit per file, so the history stays one-file-per-commit and a bisect lands on a single port.
2. **The harness stays frozen.** Every batch defines its own fixtures locally. Extending `tests/common/mod.rs` from several batches at once is the one way this phase creates conflicts; a helper moves there only when a second batch needs it.
3. **Skips are preserved, not removed.** A case bats skipped on Windows becomes `#[cfg(unix)]`; a case it skipped as root keeps that runtime check. Porting is not the moment to widen coverage — a case that could now run on Windows is noted in the receipt, not enabled here.
4. **The stream split is the one intentional strengthening.** `bats run` merged the streams; the ports name them. Where that reveals a message on the stream nobody expected, the test asserts what the binary does and the receipt records it.
5. **The CI teardown is the last task.** bats, GNU parallel, the twelve-shard matrix and `tests/test_helper.bash` go together once no `.bats` file is left; Windows then runs `cargo test` alone, unsharded, which is what the spec asked for.
6. **Batch order:** small fixtures first (Task 1), then the `sync` family (Tasks 2–3), the scaffolding commands (Tasks 4–6), the resolver and tool-config files (Task 7), the shell and release surfaces (Task 8), the recovery files (Task 9), the network stand-ins (Task 10), the teardown (Task 11). **Recommended:** as listed.

## Module closure

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result'
echo "bats files: $(ls tests/*.bats | wc -l | tr -d ' '), cases: $(grep -h -c '^@test' tests/*.bats | awk '{ s += $1 } END { print s }')"
```

Expected: `b2d2746 docs(native): close phase 7a` or later; `335`, `10`, `14`, `1`, `7`, `0`; `bats files: 38, cases: 701`.

The batches, each file one commit:

| task | bats files (cases) | total |
|---|---|---|
| 1 | enable (15), customize (13), generate (7), config_safety (7), base_skills (12), rollback (6) | 60 |
| 2 | sync (55), sync_options (17) | 72 |
| 3 | drift (28), shared (11), source_overrides (28) | 67 |
| 4 | init (37), init_flow (15), outputs_mode (10) | 62 |
| 5 | refresh (38), format_migration (11), baseline (11) | 60 |
| 6 | doctor (37), version_pin (13), workspace (7), team_workflow (8) | 65 |
| 7 | add (36), adopt (29) | 65 |
| 8 | migrate (22), simplify (16), dedupe (12) | 50 |
| 9 | profiles (20), resource_resolver (19), opencode (19) | 58 |
| 10 | release (18), guard (18), hooks (16), shell_init (15) | 67 |
| 11 | backup_retention (15), rollback_preflight (24) | 39 |
| 12 | bundle (16), update_native (11), install (9) | 36 |
| 13 | the CI teardown | — |

---

### Tasks 1–12: one batch each

**Files:** per the table. Each file: create `tests/<name>.rs`, delete `tests/<name>.bats`.

**Interfaces:** the harness of plan 7a, unchanged.

```rust
// tests/common/mod.rs
pub struct Project;
impl Project {
    pub fn empty() -> Self;                         // mktemp + git init + test identity
    pub fn seeded(init_args: &[&str]) -> Self;      // empty() then `agentsync init <args>`
    pub fn path(&self) -> &Path;
    pub fn join(&self, rel: &str) -> PathBuf;
    pub fn agentsync(&self) -> assert_cmd::Command; // cwd = project, environment scrubbed
    pub fn git(&self, args: &[&str]);
    pub fn write(&self, rel: &str, content: &str);  // creates parents
    pub fn append(&self, rel: &str, content: &str);
    pub fn read(&self, rel: &str) -> String;
    pub fn exists(&self, rel: &str) -> bool;
    pub fn sha256(&self, rel: &str) -> String;
    pub fn enable_tools(&self, tools: &[&str]);     // enable … --no-scaffold
}
pub fn scrub(command: &mut assert_cmd::Command);
pub fn unreadable_dirs_are_possible() -> bool;
#[cfg(unix)] pub fn chmod(path: &Path, mode: u32);
```

Each task repeats the same three steps, per file in its batch:

- [x] **Task 1: enable, customize, generate, config_safety, base_skills, rollback**
- [x] **Task 2: sync, sync_options**
- [x] **Task 3: drift, shared, source_overrides**
- [x] **Task 4: init, init_flow, outputs_mode**
- [x] **Task 5: refresh, format_migration, baseline**
- [x] **Task 6: doctor, version_pin, workspace, team_workflow**
- [x] **Task 7: add, adopt**
- [x] **Task 8: migrate, simplify, dedupe**
- [x] **Task 9: profiles, resource_resolver, opencode**
- [x] **Task 10: release, guard, hooks, shell_init**
- [x] **Task 11: backup_retention, rollback_preflight**
- [x] **Task 12: bundle, update_native, install**

**Step 1: Port.** Read the bats file whole. For each `@test`, write a Rust test with the same name (spaces and punctuation to `_`), asserting the same thing on the stream the binary writes it to. Fixtures the file shared through `setup`/`setup_file` become a function in the same crate. A case the Rust suite already asserts is left out and named in the commit body.

**Step 2: Verify.**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo test --test <name> 2>&1 | grep 'test result'
```

Expected: fmt exit 0, `Finished`; `N passed; 0 failed` where `N` is the ported case count for that file.

**Step 3: Commit.**

```bash
git add tests/<name>.rs tests/<name>.bats docs/plans/2026-09-19-rust-migration-phase-7b-port-the-suite.md
git commit -m "test(native): port <name>.bats"
```

The body names every retired case and the Rust test that covers it.

After the last file of a batch:

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release 2>&1 | tail -1
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: no `failed` but `0 failed`; `Finished`; only `suite-done`.

---

### Task 13: The teardown

**Files:**
- Delete: `tests/test_helper.bash`
- Modify: `.github/workflows/ci.yaml`, `.ai/src/rules/testing.md`, `.ai/src/AGENTS.md`, `README.md`, `docs/specs/2026-09-12-rust-migration-design.md`

- [x] **Step 1: Drop bats from CI**

In `.github/workflows/ci.yaml`: in `native`, delete the "Install bats-core and GNU parallel" and "The bats suite against the binary" steps; replace the whole `test-windows` job with one unsharded job that checks out, installs the toolchain, and runs `cargo test` plus `cargo build --release`; delete the sharding comment above it and the `cargo test`/`if: matrix.shard == 1` pair. Keep `lint`.

Validate: `ruby -ryaml -e 'YAML.safe_load(File.read(".github/workflows/ci.yaml"), aliases: true); puts "ci.yaml ok"'`
Expected: `ci.yaml ok`.

- [x] **Step 2: Delete the helper and rewrite the docs**

`git rm tests/test_helper.bash`. Rewrite `.ai/src/rules/testing.md` for `cargo test` alone (the harness, the naming convention, where a unit test belongs versus an integration test, the Windows rule), and the testing lines of `.ai/src/AGENTS.md`, `README.md` (Development), and the spec (Phase 7 status, Definition of Done). Run `agentsync sync --force` and `agentsync check`.

Run: `ls tests/*.bats 2>/dev/null | wc -l; grep -rn 'bats' README.md .ai/src .github/workflows/ci.yaml | wc -l; agentsync check > /dev/null; echo "check=$?"`
Expected: `0`; `0`; `check=0`.

- [x] **Step 3: Verify**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings 2>&1 | tail -1; cargo test 2>&1 | grep 'test result'
cargo build --release 2>&1 | tail -1
shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh; echo "shellcheck=$?"
```

Expected: fmt exit 0, `Finished`; every crate `0 failed`, the unit count unchanged at 335; `Finished`; `shellcheck=0`.

- [x] **Step 4: Commit**

```bash
git add -A tests .github/workflows/ci.yaml README.md .ai docs/specs/2026-09-12-rust-migration-design.md docs/plans/2026-09-19-rust-migration-phase-7b-port-the-suite.md
git commit -m "test(native): retire bats"
```

---

## Completion

The plan is closed when every box is ticked, no `.bats` file and no `tests/test_helper.bash` remain, `cargo test` carries every case the ports mapped, CI is green on Linux, macOS, and Windows with `cargo test` as the whole suite, and a `## Completion receipt` records the fresh verification. That receipt closes Phase 7 and the migration; the maintainer merges and releases.

## Completion receipt

Written 2026-09-19 on `feat/native-engine-phase-7`. This receipt closes Phase 7 and the migration.

Global Constraints:

- `.ai/src/` the source of truth, `lib/` embedded, the guard hook POSIX `sh` — untouched.
- Ported tests assert on the stream the binary writes to — the ports split what `bats run` merged; four surfaces turned out to write their refusals to stderr (`sync`'s manual-edit refusal, `check`'s version-pin error, `migrate`/`simplify`/`dedupe` usage errors, `bundle`'s export and import errors), and one splits a warning across both (`sync`'s baseline replacement: the header on stdout, the paths on stderr).
- Every case accounted for — 41 bats files, 727 cases: 723 ported one to one, 4 retired (three `cli.bats` version cases and `list alias ls works` in plan 7a; the `migrate` and `dedupe` locale-ordering cases and `migrate`'s `source.tools` refusal here, each named in its commit body).
- One bats file per commit — 41 `test(native): port <file>.bats` commits, each deleting its `.bats`.
- `tests/common/mod.rs` unchanged since plan 7a; every batch defined its fixtures locally.
- Windows — `cargo test` ran in the first Windows shard from plan 7a's first commit until this plan's teardown replaced the matrix with one `cargo test` per platform.
- Rust constraints — no dependency added; the only shells a test spawns are the ones the bats cases spawned: the guard hook, `install.sh`, a `curl`/`pbcopy`/`agentsync` stand-in on `PATH`, and a pty for the `init` wizard, each `#[cfg(unix)]`.
- ShellCheck scope — unchanged, and `lint` is the only job that still needs a shell.
- Commits Conventional, no trailers.

Fresh verification (2026-09-19):

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --all-targets -- -D warnings`: clean (two ports needed a fix: an unused `Assert` in `tests/workspace.rs` where the exit status is data, and a `let`-and-return in `tests/guard.rs`).
- `cargo test`: 1065 passed, 0 failed, across the unit tests and 41 integration crates.
- `cargo build --release`: `Finished`.
- `shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh`: exit 0.
- `ls tests/*.bats`: no matches. `tests/test_helper.bash`: deleted.
- `.github/workflows/ci.yaml` parses; its jobs are `lint` and `test` (ubuntu, macos, windows).
- `agentsync sync --force` with the new binary, then `agentsync check`: exit 0.

Skipped or deferred:

- The pty case of `tests/init.rs` cannot run inside the development agent's sandbox (`openpty` is refused there); it passes in a normal shell and in CI.
- Windows coverage is what bats had: the `#[cfg(unix)]` cases are the ones bats skipped there, with the same reasons. Widening any of them is its own change.
- `source.tools` set to an absolute directory outside the project is still not applied on Windows (open since the Phase 6 receipt); `tests/source_overrides.rs` keeps the case `#[cfg(unix)]`.
- Two assertions inherited from bats are weak and were ported as they stood: `drift`'s manifest check passes on a missing manifest, and `source_overrides`' `git diff` check cannot see untracked paths. Noted rather than strengthened, so the port stays a translation.
- `Please run: lib/sync.sh` in the `check` report still names the retired Bash entry point. It is the one user-visible string left from the old engine; changing it is an accepted deviation for a later release, not a port.

### What the new matrix found (2026-09-19, after the receipt)

Until this phase, `cargo test` ran on Linux and macOS only, and clippy never ran on Windows at all: the Windows gate was bats. Turning the matrix into one `cargo test` per platform surfaced seven findings in six runs, one of them an engine bug. CI is green on all four jobs at `77168f2` (run 35459770928).

| finding | where | why it hid |
|---|---|---|
| `find_parent_ai_src` spun forever when no `.git` exists anywhere: its walk-up stopped at `"/"`, and on Windows the root is `C:/`, whose parent is itself | `src/paths.rs` | every bats fixture ran `git init`, so the walk always hit a repository boundary first |
| a unit test built its expected message with `Path::display`, whose separator is wrong on Windows | `src/project.rs` | unit tests had never run on Windows |
| helpers and imports serving only `#[cfg(unix)]` cases are dead code | nine sites in `tests/` and two in `src/` | `clippy --all-targets` had never run on Windows |
| `dedupe`'s fixture ran `init` with an inherited stdin, which the runner's console reads as a terminal | `tests/dedupe.rs` | bats never inherited a console |
| `set_modified` needs a handle open for writing, and a directory cannot be opened for writing at all | `tests/backup_retention.rs` | the engine hit the same trap in Phase 6; the fixture had not |
| a trust root must arrive canonicalised: the runner's `TEMP` is the 8.3 name `RUNNER~1`, and a path list splits on `;` | `tests/source_overrides.rs` | `host_path` (`cygpath -ml`) did both for bats |
| writing a payload to a hook that exits without reading it fails with `EPIPE` | `tests/guard.rs` | a race macOS won and Ubuntu lost; unrelated to Windows |

`cargo clippy --all-targets --target x86_64-pc-windows-msvc` catches the dead-code class on the development host, which is how the nine sites were found in one pass rather than one CI run each. CI runs `cargo test --no-fail-fast` so a run reports every failing crate.

## Run log


### 2026-09-19 — phase closed, migration complete
- Commits: 41 `test(native): port <file>.bats`, from 350ff60 (cli) to ffb93ac (rollback_preflight); 0a44d54 style(native): return the fixture project directly; 72b55dc test(native): retire bats; this commit, docs(native): close phase 7.
- Verified: `cargo fmt --all --check` exit 0; `cargo clippy --all-targets -- -D warnings` clean; `cargo test` 1065 passed, 0 failed; `cargo build --release` `Finished`; shellcheck on the two scripts exit 0; no `.bats` file and no `tests/test_helper.bash`; `ci.yaml` parses with jobs `lint` and `test`; `agentsync sync --force` and `agentsync check` exit 0.
- Plan amended: the batch list in the task table is the one that ran; the twelve porting tasks were executed in parallel, one agent per batch, and every port was verified with `cargo test --test <stem>` before its commit.
- Next: the maintainer pushes and reads CI, then merges `feat/native-engine-phase-7` into `main` and runs `agentsync release minor` (0.38.0, the first release with no Bash engine and no bats).
- Blocker: none.
