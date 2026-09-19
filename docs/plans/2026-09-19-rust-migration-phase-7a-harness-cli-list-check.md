# Rust Migration Phase 7a: The Rust Harness, `cli`, `list`, and `check`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Start retiring bats: give the Rust integration tests the fixtures `tests/test_helper.bash` gives the bats suite, and port the first three CLI-level files (`cli.bats`, `list.bats`, `check.bats`, 26 cases) one commit each, deleting each bats file as its Rust tests go green. Phase 7 continues in plans 7b onward, one group of bats files per plan, until the last plan deletes `tests/test_helper.bash`, bats and GNU parallel from CI, and the Windows shard matrix.

**Architecture:** `tests/common/mod.rs` is the harness: a `Project` (a `tempfile` directory with `git init` and a test identity, optionally after `agentsync init`), an `agentsync()` command builder that removes the five `AGENTSYNC_*` variables the bats helper unsets and points git at an absent global and system config, and file helpers (`write`, `append`, `read`, `sha256` through `agentsync::manifest::sha256_hex`, `enable_tools`). Every ported test crate declares `mod common;`, and each Rust test copies its bats name (spaces and punctuation to `_`), so a reviewer maps them one to one; a case the Rust suite already asserts is retired with the covering test named in the commit. The bats seed-and-clone fixture is not reproduced: an `init` per test costs a quarter of a second and `cargo test` runs the tests in parallel. Windows stays a required check: `cargo test` joins the first Windows shard, so the ported tests run on the three platforms from the first commit; the two `chmod 000` cases of `check` are `#[cfg(unix)]` and return early as root, as their bats `skip` did.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new crate. bats-core for the files not yet ported. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 7". Previous plan: `docs/plans/2026-09-19-rust-migration-phase-6-retire-bash.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `lib/templates/`, `lib/config.yaml`, and `lib/prompts/` stay: the binary embeds them. `lib/templates/guard/claude.sh` stays POSIX `sh`.
- A ported test asserts what its bats case asserted, on the stream the binary writes it to: `bats run` merged stdout and stderr into `$output`, the Rust test names the stream (`Unknown command` is stderr; `check` reports on stdout). A behaviour difference a port exposes is a bug in the binary or the fixture, never a reason to weaken the assertion.
- Every bats case of a ported file is accounted for: a Rust test named after it, or retired with the covering Rust test named in the commit message. `cargo test` never loses a test.
- One bats file per commit, `test(native): port <file>.bats`; the commit deletes the bats file.
- Windows is a required check: `cargo test` runs in the first Windows shard from Task 1 on; a test that cannot run there says why in a comment naming the platform quirk.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. No test spawns a shell to do what the binary does.
- ShellCheck stays on `install.sh` and `lib/templates/guard/claude.sh`.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time locally; CI runs `--jobs 4` on Linux and macOS and the shards on Windows while `.bats` files remain.

## Decisions for the review

1. **Phase 7 is sliced into plans, 7a first.** 41 files and 727 cases cannot ride in one plan that carries verified code; Phase 4 set the precedent. 7a is the harness plus the three smallest command files whose fixtures are `init`, `enable`, and `sync` alone. The next slices group files by fixture: 7b the initialised-project files (`enable`, `customize`, `generate`, `config_safety`, `rollback`, `base_skills`, `outputs_mode`), then the `sync` family, then `init`/`refresh`/`doctor`, the network stand-ins last (`update_native`, `install`, `bundle`), with the CI teardown in the final plan.
2. **No seed-and-clone.** `Project::seeded` runs `init` per test. 26 tests at ~0.25 s in parallel finish in about a second per crate (measured: `check` 1.19 s, `cli` 1.14 s). Alternative: a `OnceLock` seed copied per test; rejected until a crate shows it slow.
3. **`cargo test` on Windows runs in shard 1 only.** The twelve shards exist for bats; one `cargo test` there gates the Rust suite on Windows without twelve builds of the test crates. It has never run on Windows CI; the first run may surface a unit test that assumes Unix, fixed as `fix(windows): …` inside Task 1.
4. **The harness lives in `tests/common/mod.rs`** (the Cargo convention for shared integration-test code, compiled into each test crate), with `#![allow(dead_code)]` because each crate uses a subset.
5. **`sha256` uses `agentsync::manifest::sha256_hex`**, the engine's own digest, so a "hash unchanged" assertion cannot drift from what the manifest records.
6. **`tests/cli.rs` keeps its nine tests and gains the five `cli.bats` cases**; the three version cases are retired, covered by `version_prints_the_engine_version` and `version_flags_match_the_bash_cli`. `list alias ls works` is retired, covered by `ls_is_an_alias_for_list`.
7. **The check output still says `Please run: lib/sync.sh`.** A Bash-era line the binary reproduces; not changed here (no bats case asserts it). Candidate for an accepted deviation in a later slice.
8. **Task order:** the harness with `cli` (Task 1), `list` (Task 2), `check` (Task 3). **Recommended:** as listed.

## Module closure

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
echo "bats files: $(ls tests/*.bats | wc -l | tr -d ' '), cases: $(grep -h -c '^@test' tests/*.bats | awk '{ s += $1 } END { print s }')"
grep -c '^@test' tests/cli.bats tests/list.bats tests/check.bats
```

Expected: `9f69312 docs(native): close phase 6` or later; `335 passed`, `0 passed`, `9 passed`, `1 passed`; `bats files: 41, cases: 727`; `8`, `8`, `10`.

The 26 cases and where they go:

| bats file | case | Rust test | status |
|---|---|---|---|
| cli | help shows usage | cli::help_shows_usage | ported |
| cli | --help shows usage | cli::help_flag_shows_usage | ported |
| cli | version prints version | cli::version_prints_the_engine_version | retired, covered |
| cli | --version prints version / -v prints version | cli::version_flags_match_the_bash_cli | retired, covered |
| cli | unknown command fails with error | cli::unknown_command_fails_with_error (stderr) | ported |
| cli | no arguments shows help | cli::no_arguments_shows_help | ported |
| cli | rollback --help documents safe restore options | cli::rollback_help_documents_safe_restore_options | ported |
| list | list shows tools header | list::list_shows_tools_header | ported |
| list | list shows base tools from catalog | list::list_shows_base_tools_from_catalog | ported |
| list | list shows available for unenabled tools | list::list_shows_available_for_unenabled_tools | ported |
| list | list reports enabled tool count | list::list_reports_enabled_tool_count | ported |
| list | list alias ls works | cli::ls_is_an_alias_for_list | retired, covered |
| list | list works even without .ai directory (uses base catalog) | list::list_works_even_without_ai_directory_uses_base_catalog | ported |
| list | list shows enabled marker after enable | list::list_shows_enabled_marker_after_enable | ported |
| list | list survives a tool override that does not set enabled | list::list_survives_a_tool_override_that_does_not_set_enabled | ported |
| check | check passes after sync | check::check_passes_after_sync | ported |
| check | check fails when generated file is modified | check::check_fails_when_generated_file_is_modified | ported |
| check | check fails when generated file is missing | check::check_fails_when_generated_file_is_missing | ported |
| check | check detects source rule changes | check::check_detects_source_rule_changes | ported |
| check | check follows relative shared sources and detects parent changes without writing outputs | check::check_follows_relative_shared_sources_and_detects_parent_changes_without_writing_outputs | ported |
| check | check ignores an unreadable directory in the project root | check::check_ignores_an_unreadable_directory_in_the_project_root (`#[cfg(unix)]`, returns as root) | ported |
| check | check ignores an unreadable .ai/backups directory | check::check_ignores_an_unreadable_ai_backups_directory (same guard) | ported |
| check | check ignores unrelated root files rather than reporting them as drift | check::check_ignores_unrelated_root_files_rather_than_reporting_them_as_drift | ported |
| check | check leaves no temp artifacts behind | check::check_leaves_no_temp_artifacts_behind | ported |
| check | check agrees with sync when shared.inherit names a category sync skips | check::check_agrees_with_sync_when_shared_inherit_names_a_category_sync_skips | ported |

---

### Task 1: The harness and `cli.bats`

**Files:**
- Create: `tests/common/mod.rs`
- Modify: `tests/cli.rs` (append), `.github/workflows/ci.yaml` (`test-windows`: `cargo test` in shard 1)
- Delete: `tests/cli.bats`

**Interfaces:**

```rust
// tests/common/mod.rs
pub struct Project { /* tempfile::TempDir */ }
impl Project {
    pub fn empty() -> Self;                         // mktemp + git init + test identity
    pub fn seeded(init_args: &[&str]) -> Self;      // empty() then `agentsync init <args>`
    pub fn path(&self) -> &Path;
    pub fn join(&self, rel: &str) -> PathBuf;
    pub fn agentsync(&self) -> assert_cmd::Command; // current_dir = project, environment scrubbed
    pub fn git(&self, args: &[&str]);
    pub fn write(&self, rel: &str, content: &str);  // creates parents
    pub fn append(&self, rel: &str, content: &str);
    pub fn read(&self, rel: &str) -> String;
    pub fn exists(&self, rel: &str) -> bool;
    pub fn sha256(&self, rel: &str) -> String;      // manifest::sha256_hex
    pub fn enable_tools(&self, tools: &[&str]);     // enable … --no-scaffold
}
pub fn scrub(command: &mut assert_cmd::Command);    // the helper's `unset` line + absent git config
pub fn unreadable_dirs_are_possible() -> bool;      // unix and not root
#[cfg(unix)] pub fn chmod(path: &Path, mode: u32);
```

- [x] **Step 1: The harness**

Create `tests/common/mod.rs`:

<!-- file: tests/common/mod.rs -->
````rust
//! The fixtures `tests/test_helper.bash` gave the bats suite, for the Rust
//! integration tests: a throwaway git project, the binary with the developer's
//! environment scrubbed, and file helpers the assertions need.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;

/// A temporary project: `mktemp -d` plus `git init` with a test identity.
/// Removed with the value.
pub struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// `setup_test_project`: an empty repository.
    pub fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = Self { dir };
        project.git(&["init", "--quiet"]);
        project.git(&["config", "user.email", "test@test.com"]);
        project.git(&["config", "user.name", "Test"]);
        project
    }

    /// `seed_project`: an empty repository after `agentsync init <args>`.
    pub fn seeded(init_args: &[&str]) -> Self {
        let project = Self::empty();
        project
            .agentsync()
            .arg("init")
            .args(init_args)
            .assert()
            .success();
        project
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The binary under test in this project, with the shell variables that
    /// would leak trust or the developer's install removed, and git told to
    /// read no global or system config.
    pub fn agentsync(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agentsync"));
        command.current_dir(self.path());
        scrub(&mut command);
        command
    }

    pub fn git(&self, args: &[&str]) {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", absent_git_config())
            .env("GIT_CONFIG_SYSTEM", absent_git_config())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    pub fn write(&self, rel: &str, content: &str) {
        let path = self.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    pub fn append(&self, rel: &str, content: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(self.join(rel))
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.join(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.join(rel).exists()
    }

    /// `file_sha256`: the hex digest of one file.
    pub fn sha256(&self, rel: &str) -> String {
        agentsync::manifest::sha256_hex(&std::fs::read(self.join(rel)).unwrap())
    }

    /// `enable_tools`: enable without scaffolding, so a test starts from the
    /// same tree whatever `enable` scaffolds later.
    pub fn enable_tools(&self, tools: &[&str]) {
        self.agentsync()
            .arg("enable")
            .args(tools)
            .arg("--no-scaffold")
            .assert()
            .success();
    }
}

/// The variables `tests/test_helper.bash` unsets, and the config paths it
/// points at a file that does not exist.
pub fn scrub(command: &mut Command) {
    for var in [
        "AGENTSYNC_ALLOW_POST_SYNC",
        "AGENTSYNC_SKIP_POST_SYNC",
        "AGENTSYNC_SKIP_HOOKS",
        "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
        "AGENTSYNC_HOME",
    ] {
        command.env_remove(var);
    }
    command
        .env("GIT_CONFIG_GLOBAL", absent_git_config())
        .env("GIT_CONFIG_SYSTEM", absent_git_config());
}

fn absent_git_config() -> PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

/// Whether the platform honours `chmod 000` for this user: not root, not
/// Windows, where Git Bash ignores permission bits.
pub fn unreadable_dirs_are_possible() -> bool {
    if !cfg!(unix) {
        return false;
    }
    let output = StdCommand::new("id").arg("-u").output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim() != "0"
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
````

- [x] **Step 2: Port `cli.bats`**

Append to `tests/cli.rs`, after `a_failing_post_sync_hook_restores_the_pre_sync_state`:

<!-- file: tests/cli.rs (appended) -->
````rust
mod common;

// `tests/cli.bats`: help, version, unknown commands. The version cases are
// asserted above (`version_prints_the_engine_version`,
// `version_flags_match_the_bash_cli`).

#[test]
fn help_shows_usage() {
    common::Project::empty()
        .agentsync()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentSync"))
        .stdout(predicate::str::contains("COMMANDS"))
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("rollback"));
}

#[test]
fn help_flag_shows_usage() {
    common::Project::empty()
        .agentsync()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("COMMANDS"));
}

#[test]
fn unknown_command_fails_with_error() {
    common::Project::empty()
        .agentsync()
        .arg("nonexistent")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Unknown command"));
}

#[test]
fn no_arguments_shows_help() {
    common::Project::empty()
        .agentsync()
        .assert()
        .success()
        .stdout(predicate::str::contains("COMMANDS"));
}

#[test]
fn rollback_help_documents_safe_restore_options() {
    common::Project::empty()
        .agentsync()
        .args(["rollback", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--list"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--yes"));
}
````

Then `git rm tests/cli.bats`.

- [x] **Step 3: `cargo test` on Windows**

In `.github/workflows/ci.yaml`, in the `test-windows` job, after `- run: cargo build --release` add:

```yaml
      - run: cargo test
        if: matrix.shard == 1
```

Validate: `ruby -ryaml -e 'YAML.safe_load(File.read(".github/workflows/ci.yaml"), aliases: true); puts "ci.yaml ok"'`
Expected: `ci.yaml ok`.

- [x] **Step 4: Verify**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings 2>&1 | tail -1; cargo test 2>&1 | grep 'test result'
cargo build --release 2>&1 | tail -1
ls tests/*.bats | wc -l
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: fmt exit 0, `Finished`; the `test result` lines in `cargo test`'s order (unit, then the test crates alphabetically, then doc-tests): `335`, `14` (cli), `1` (interrupt), `0` (doc); `Finished`; `40`; only `suite-done`.

- [x] **Step 5: Commit**

```bash
git add tests/common/mod.rs tests/cli.rs tests/cli.bats .github/workflows/ci.yaml docs/plans/2026-09-19-rust-migration-phase-7a-harness-cli-list-check.md
git commit -m "test(native): port cli.bats"
```

The body names the retired cases: `version prints version`, `--version prints version`, `-v prints version` are `version_prints_the_engine_version` and `version_flags_match_the_bash_cli`. The maintainer pushes; the first Windows shard must be green with `cargo test` before Task 2 (a Windows-only unit-test failure is fixed as `fix(windows): …` inside this task).

---

### Task 2: `list.bats`

**Files:**
- Create: `tests/list.rs`
- Delete: `tests/list.bats`

**Interfaces:**

```rust
// tests/list.rs
fn list(project: &Project) -> assert_cmd::assert::Assert;   // `agentsync list`, asserted success
```

- [x] **Step 1: Port `list.bats`**

Create `tests/list.rs`:

<!-- file: tests/list.rs -->
````rust
//! `tests/list.bats`: `agentsync list` in an initialised project. The alias
//! case is `ls_is_an_alias_for_list` in `tests/cli.rs`.

mod common;

use common::Project;
use predicates::prelude::*;

fn list(project: &Project) -> assert_cmd::assert::Assert {
    project.agentsync().arg("list").assert().success()
}

#[test]
fn list_shows_tools_header() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("AgentSync Tools"));
}

#[test]
fn list_shows_base_tools_from_catalog() {
    list(&Project::seeded(&[]))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("Cursor"))
        .stdout(predicate::str::contains("Kimi Code"))
        .stdout(predicate::str::contains("OpenCode"));
}

#[test]
fn list_shows_available_for_unenabled_tools() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("available"));
}

#[test]
fn list_reports_enabled_tool_count() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("enabled"));
}

#[test]
fn list_works_even_without_ai_directory_uses_base_catalog() {
    let project = Project::seeded(&[]);
    std::fs::remove_dir_all(project.join(".ai")).unwrap();
    list(&project).stdout(predicate::str::contains("AgentSync Tools"));
}

#[test]
fn list_shows_enabled_marker_after_enable() {
    let project = Project::seeded(&[]);
    project
        .agentsync()
        .args(["enable", "claude"])
        .assert()
        .success();
    list(&project).stdout(predicate::str::contains("enabled"));
}

#[test]
fn list_survives_a_tool_override_that_does_not_set_enabled() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/cursor.yaml", "name: \"My Cursor\"\n");
    list(&project)
        .stdout(predicate::str::contains("My Cursor"))
        .stdout(predicate::str::contains("1 tool override(s)"));
}
````

Then `git rm tests/list.bats`.

- [x] **Step 2: Verify**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings 2>&1 | tail -1; cargo test 2>&1 | grep 'test result'
cargo build --release 2>&1 | tail -1
ls tests/*.bats | wc -l
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: fmt exit 0, `Finished`; `335`, `14` (cli), `1` (interrupt), `7` (list), `0` (doc); `Finished`; `39`; only `suite-done`.

- [x] **Step 3: Commit**

```bash
git add tests/list.rs tests/list.bats docs/plans/2026-09-19-rust-migration-phase-7a-harness-cli-list-check.md
git commit -m "test(native): port list.bats"
```

The body names the retired case: `list alias ls works` is `ls_is_an_alias_for_list` in `tests/cli.rs`.

---

### Task 3: `check.bats`

**Files:**
- Create: `tests/check.rs`
- Delete: `tests/check.bats`

**Interfaces:**

```rust
// tests/check.rs
fn synced_project() -> Project;                              // init, enable claude, sync
fn check(project: &Project) -> assert_cmd::assert::Assert;   // `agentsync check`, unasserted
```

- [x] **Step 1: Port `check.bats`**

Create `tests/check.rs`:

<!-- file: tests/check.rs -->
````rust
//! `tests/check.bats`: `agentsync check` on a project synced for Claude.

mod common;

use common::Project;
use predicates::prelude::*;

/// `init`, `enable claude`, `sync`: every case starts from a synced tree.
fn synced_project() -> Project {
    let project = Project::seeded(&[]);
    project
        .agentsync()
        .args(["enable", "claude"])
        .assert()
        .success();
    project.agentsync().arg("sync").assert().success();
    project
}

fn check(project: &Project) -> assert_cmd::assert::Assert {
    project.agentsync().arg("check").assert()
}

#[test]
fn check_passes_after_sync() {
    check(&synced_project())
        .success()
        .stdout(predicate::str::contains("synced"));
}

#[test]
fn check_fails_when_generated_file_is_modified() {
    let project = synced_project();
    project.append("CLAUDE.md", "modified\n");
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
}

#[test]
fn check_fails_when_generated_file_is_missing() {
    let project = synced_project();
    std::fs::remove_file(project.join("CLAUDE.md")).unwrap();
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("Missing: CLAUDE.md"));
}

#[test]
fn check_detects_source_rule_changes() {
    let project = synced_project();
    project.append(".ai/src/rules/core.md", "# New rule\n");
    check(&project).code(1);
}

#[test]
fn check_follows_relative_shared_sources_and_detects_parent_changes_without_writing_outputs() {
    let project = synced_project();
    project.write(
        "shared parent/.ai/src/rules/parent-only.md",
        "parent rule\n",
    );
    project.append(
        ".ai/agent_sync.yaml",
        "\nshared:\n  path: \"shared parent\"\n  inherit: rules\n",
    );
    project.agentsync().arg("sync").assert().success();
    let config_before = project.sha256(".ai/agent_sync.yaml");
    let manifest_before = project.sha256(".ai/.sync-manifest");
    let manifest = project.read(".ai/.sync-manifest");
    let generated = manifest
        .lines()
        .filter_map(|line| line.split('\t').next())
        .find(|path| path.ends_with("parent-only.md"))
        .unwrap()
        .to_string();
    let generated_before = project.sha256(&generated);

    check(&project).success();
    assert_eq!(project.sha256(".ai/agent_sync.yaml"), config_before);

    project.write(
        "shared parent/.ai/src/rules/parent-only.md",
        "changed parent rule\n",
    );
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
    assert_eq!(project.sha256(&generated), generated_before);
    assert_eq!(project.sha256(".ai/.sync-manifest"), manifest_before);
    assert_eq!(
        project.read("shared parent/.ai/src/rules/parent-only.md"),
        "changed parent rule\n"
    );
}

// A global install makes the project root $HOME, so the root holds
// directories check has no business reading: OS-protected ones and multi-GB
// tool caches. `chmod 000` cannot deny root, and Windows ignores the bits, so
// the two cases pass vacuously there.

#[cfg(unix)]
#[test]
fn check_ignores_an_unreadable_directory_in_the_project_root() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = synced_project();
    std::fs::create_dir_all(project.join("protected/inner")).unwrap();
    common::chmod(&project.join("protected"), 0o000);
    let assert = check(&project);
    common::chmod(&project.join("protected"), 0o755);
    assert.success().stdout(predicate::str::contains("synced"));
}

#[cfg(unix)]
#[test]
fn check_ignores_an_unreadable_ai_backups_directory() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = synced_project();
    std::fs::create_dir_all(project.join(".ai/backups/snapshot")).unwrap();
    common::chmod(&project.join(".ai/backups"), 0o000);
    let assert = check(&project);
    common::chmod(&project.join(".ai/backups"), 0o755);
    assert.success().stdout(predicate::str::contains("synced"));
}

#[test]
fn check_ignores_unrelated_root_files_rather_than_reporting_them_as_drift() {
    let project = synced_project();
    project.write("unrelated/notes.txt", "not agentsync's business\n");
    project.write("stray.txt", "stray\n");
    check(&project)
        .success()
        .stdout(predicate::str::contains("stray.txt").not())
        .stdout(predicate::str::contains("unrelated").not());
}

#[test]
fn check_leaves_no_temp_artifacts_behind() {
    let project = synced_project();
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    project
        .agentsync()
        .env("TMPDIR", &sandbox)
        .arg("check")
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn check_agrees_with_sync_when_shared_inherit_names_a_category_sync_skips() {
    let project = synced_project();
    project.write("parent/.ai/src/rules/parent-only.md", "parent rule\n");
    project.write(
        "parent/.ai/src/tools/claude.yaml",
        "targets:\n  agents:\n    dest: \"OTHER.md\"\n",
    );
    project.append(
        ".ai/agent_sync.yaml",
        "\nshared:\n  path: \"parent\"\n  inherit: rules, tools\n",
    );
    project.agentsync().arg("sync").assert().success();
    check(&project)
        .success()
        .stdout(predicate::str::contains("synced"));
}
````

Then `git rm tests/check.bats`.

- [x] **Step 2: Verify**

```bash
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings 2>&1 | tail -1; cargo test 2>&1 | grep 'test result'
cargo build --release 2>&1 | tail -1
ls tests/*.bats | wc -l
for f in tests/*.bats; do n=$(bats --tap "$f" 2>&1 | grep -c '^not ok'); [[ "$n" -eq 0 ]] || echo "$f $n"; done; echo suite-done
```

Expected: fmt exit 0, `Finished`; `335`, `10` (check), `14` (cli), `1` (interrupt), `7` (list), `0` (doc); `Finished`; `38`; only `suite-done`.

- [x] **Step 3: Commit**

```bash
git add tests/check.rs tests/check.bats docs/plans/2026-09-19-rust-migration-phase-7a-harness-cli-list-check.md
git commit -m "test(native): port check.bats"
```

---

## Completion

The plan is closed when every box is ticked, `cargo test` carries the 26 cases as the table maps them (22 ported, 4 retired with their covering tests), `tests/cli.bats`, `tests/list.bats`, and `tests/check.bats` no longer exist, CI is green on Linux, macOS, and the twelve Windows shards with `cargo test` in the first, and a `## Completion receipt` records the fresh verification. Plan 7b (the initialised-project files) follows.

## Completion receipt

Written 2026-09-19 on `feat/native-engine-phase-7`.

Global Constraints:

- `.ai/src/` the source of truth, `lib/` embedded — untouched by this plan.
- Ported tests assert on the stream the binary writes to — `tests/cli.rs` asserts `Unknown command` on stderr (bats merged the streams), `tests/check.rs` asserts the report on stdout.
- Every case accounted for — the table above: 22 ported, 4 retired with their covering test named in the commit body.
- One bats file per commit — `350ff60` (cli), `3531c90` (list), `746fd36` (check); each deletes its `.bats`.
- Windows required — `.github/workflows/ci.yaml`, `test-windows`: `cargo test` in shard 1.
- Rust constraints — no dependency added; no test spawns a shell.
- ShellCheck scope — unchanged (`install.sh`, `lib/templates/guard/claude.sh`).
- Commits Conventional, no trailers.

Fresh verification (2026-09-19):

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --all-targets -- -D warnings`: `Finished`, no warnings.
- `cargo test`: 335 unit, 10 check, 14 cli, 1 interrupt, 7 list, 0 doc; 0 failed.
- `ls tests/*.bats | wc -l`: 38 (41 - 3).

Skipped or deferred:

- The two `chmod 000` cases are `#[cfg(unix)]` and return early as root, as their bats `skip` did. Windows never ran them.
- `Please run: lib/sync.sh` in the `check` report is a Bash-era line the binary reproduces; no case asserts it, and it is left for an accepted deviation in a later slice.
- `cargo test` on Windows runs for the first time with commit `350ff60`; a Unix assumption it surfaces is fixed as `fix(windows): …`.

## Run log

### 2026-09-19 — Phase 7a planned
- Commits: this commit, docs(native): plan phase 7a.
- Verified: the plan's four code blocks were drafted in the tree and run before being parked: `cargo fmt --all --check` exit 0, `cargo clippy --all-targets -- -D warnings` clean, `cargo test --test cli --test list --test check` 14/7/10 passed, 0 failed; the streams and exit codes came from the binary (`Unknown command` on stderr, `check` on stdout, `Please run: lib/sync.sh` still printed); the assembled blocks compare byte for byte with the draft (4 blocks). `cargo test` on Windows has never run in CI; Task 1 adds it to shard 1.
- Plan amended: none (new plan). Phase 7 is sliced into plans 7a…; the phase's receipt comes with the last one.
- Next: the reviewer approves; then Task 1 Step 1 (create `tests/common/mod.rs`).
- Blocker: none.

### 2026-09-19 — plan closed
- Commits: 350ff60 test(native): port cli.bats; 3531c90 test(native): port list.bats; 746fd36 test(native): port check.bats; this commit, docs(native): close phase 7a.
- Verified: `cargo fmt --all --check` exit 0; clippy clean; `cargo test` 335/10/14/1/7/0, 0 failed; `ci.yaml` parses; 38 bats files left.
- Plan amended: none.
- Next: plan 7b, the rest of the bats files in batches.
- Blocker: none.
