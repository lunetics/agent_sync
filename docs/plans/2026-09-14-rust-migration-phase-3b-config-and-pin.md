# Rust Migration Phase 3b, Family 1: Config Selection and the Version Pin

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the native `sync`, `check`, and `list` select `agent_sync.yaml`, refuse a configless sync, and enforce `version_pin.mode` exactly as release 0.36.0's `lib/sync.sh`, `lib/check.sh`, `lib/helpers/project_config.sh`, and `lib/helpers/version.sh` do.

**Architecture:** Two Tier 1 modules mirror the Bash helpers: `src/project_config.rs` answers `project_config_path_r` over an `is_file` probe, so the in-memory `check` workspace, the on-disk `sync` workspace, and `list`'s `Project` share one rule; `src/version.rs` answers `version_pin_mode` and the mismatch message. `render::load_run_config` stops on a missing explicit path and on an unknown pin mode instead of warning; `render::refuse_configless_cleanup` runs where `lib/sync.sh` calls `_refuse_configless_cleanup_or_exit`; `cli::check` selects the config up front, seeds it into its workspace when it lives outside `.ai/`, and applies the same committed-or-strict rule as `sync`. The test seam stays the CLI process boundary: `tests/native_parity.bats` compares both engines byte for byte, and each module's unit tests assert the values captured from the Bash helpers on 2026-09-14.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Bash 3.2 and bats-core for conformance. Design: `docs/specs/2026-09-12-rust-migration-design.md`, section "Phase 3b — Bash 0.36.0 behaviour in `sync`, `check`, `list`, and `rollback`", family 1. Previous plan: `docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md` (closed, receipt 2026-09-14).

## Global Constraints

- `.ai/src/` remains the source of truth. The native `sync` writes only what `lib/sync.sh` writes; `check` and `list` write nothing.
- No binary ships to users in this phase: without a built binary every command runs in Bash exactly as today.
- `bin/agentsync.sh` and `lib/**/*.sh` stay Bash 3.2-compatible and clean under `shellcheck -x -S warning -e SC1091`. This family changes no Bash.
- A ported command matches Bash byte for byte on stdout, stderr, exit status, and the files it leaves when stdout is not a terminal, except for the accepted deviations; on a terminal the log's escape codes are those of `lib/helpers/logging.sh`.
- Rust: `unsafe_code = "forbid"`; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean after every task; no YAML or JSON crate; no new dependency.
- Unit tests that touch the disk are `#[cfg(unix)]`; everything else runs on an in-memory `Workspace` rooted at `/proj`, so `cargo test` stays green on the Windows runner.
- `VERSION` is the only version source; `Cargo.toml` keeps `0.0.0` until Phase 5.
- Accepted deviations already in the spec (Phases 1 to 3) still hold, except the Phase 2 line about `check` and the `shared:` block, which Task 6 retires: since `8677ed9` Bash reads the block in place as the binary does.
- Known quirk reproduced and appended to the spec in Task 6: 13 (`version_pin: warn` before a `version_pin:` mapping yields `warn`, because the reader answers the first `version_pin` key).
- Every expected message below was captured from Bash 0.36.0 on 2026-09-14 by `scratchpad/phase3b/bash_reference.sh`; a unit test asserts that text, never one derived from reading the helper.
- Commits follow Conventional Commits, scope `native` for engine work, imperative subject, no attribution trailers.

## Decisions for the review

Implementation waits for these. Each has a recommendation, and the tasks are written against it.

Taken on 2026-09-14: all four as recommended. No dependency is added.

1. **Phase 3b as four plans.** The spec section lists four families (config and pin, retention, outside sources, rollback witness) with disjoint modules. One plan for all four would be the size of Phase 3's (8 769 lines) and reviewable only as a whole. **Recommended:** one plan per family, in the spec's order, each closed with its own receipt; `native-next` treats Phase 3b as closed when all four are. Alternative: a single Phase 3b plan.
2. **`src/version.rs` for the pin policy.** The module map already names `src/version.rs` as the home of `lib/helpers/version.sh`; `engine_version` stays in `src/lib.rs`, where every command reads it. **Recommended:** create `src/version.rs` with `Mode`, `mode`, `mismatch_error`, and `hint`, and let `render` and `cli::check` share it. Alternative: keep the two copies of the hint text in `render.rs` and `cli/check.rs`.
3. **Quirk 13.** `version_pin: warn` followed later by a `version_pin:` mapping with `mode: strict` answers `warn` in Bash. **Recommended:** reproduce it (it follows from `yaml_subset` mirroring `yaml.sh`) and number it 13. Alternative: none that keeps `yaml_subset` a mirror.
4. **`list` and a missing explicit config.** Bash's `list` now exits 1 with `Error: AGENTSYNC_CONFIG_PATH is set but file not found: <path>`. **Recommended:** a new `Error::ConfigPathNotFound`, whose `Display` is that sentence, so `main` prints it with the existing `Error:` prefix and status 1.

## Module closure

```text
lib/helpers/project_config.sh     1-24      project_config_path_r
lib/helpers/version.sh            23-63     version_pin_mode, version_pin_mismatch_error, version_pin_mismatch_hint
lib/sync.sh                       159-165   resolve_project_config_path
                                  733-802   _load_run_config: selection, version_pin mode before defaults and outputs
                                  804-813   _refuse_configless_cleanup_or_exit
                                  833-850   _check_version_pin_or_exit: committed or strict
                                  1305-1310 main: if-stale, configless refusal, pin
lib/check.sh                      28-68     selection before anything prints, _check_version_pin with gitignore.update
                                  164-176   isolated sync receives the absolute config path
lib/helpers/tool_resolver.sh      47-57     tool_resolver_select_project_config (list's message)
lib/helpers/list.sh               13-24     _list_prepare_context
```

Out of this family, as the spec orders: `backup.retention`, `source.*` outside the project, symlink containment, and the rollback witness. `tests/native_parity.bats` keeps one red fixture in both modes until family 4: `parity: rollback plans, restores, and refuses like Bash`, whose Bash usage text now names `--force`.

---

### Task 0: Baseline

**Files:**
- None changed.

**Interfaces:**
- Consumes: branch `feat/native-engine-phase-1` at `8677ed9` or later, with `main` (0.36.0) merged; `cargo` on `PATH` (or `~/.cargo/bin/cargo`).
- Produces: recorded counts to measure the family against.

- [x] **Step 1: Confirm the branch and the toolchain**

```bash
git branch --show-current
git status --short
git log --oneline -3
cargo --version
bats --version
```

Expected: `feat/native-engine-phase-1`; an empty status apart from `?? target/`; `8677ed9 fix(check): let the isolated sync inherit shared sources itself` in the log; `cargo 1.85` or newer; `Bats 1.5` or newer.

- [x] **Step 2: Record the baseline**

Run the bats files one at a time: the full suite with `--jobs 6` exhausted memory on the development machine on 2026-09-14.

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/config_safety.bats | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/version_pin.bats | grep '^not ok'
AGENTSYNC_NATIVE=0 bats --tap tests/config_safety.bats tests/version_pin.bats | grep -c '^not ok'
for f in check list sync outputs_mode team_workflow workspace; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `147 passed` (unit) and `11 passed` (integration); `5` native failures in `config_safety.bats`; exactly these native failures in `version_pin.bats`:

```text
not ok 5 version pin: the scalar shorthand makes local mode strict
not ok 7 version pin: local mode can be made strict for sync
not ok 8 version pin: local strict mode also fails check
not ok 9 version pin: unknown mode fails before writing
not ok 10 version pin: check rejects an unknown mode
```

and `0` Bash failures. The six per-file native counts are the baseline Tasks 3 to 6 measure against: record them in the plan's Run log. `check` and `list` were `0` on 2026-09-14; the other four were not yet measured after the merge, and a non-zero count there that this family does not touch stays out of scope and goes into the Run log as found.

---

### Task 1: `project_config_path_r` as `src/project_config.rs`

**Files:**
- Create: `src/project_config.rs`
- Modify: `src/lib.rs` (module list)

**Interfaces:**
- Consumes: nothing beyond `std`.
- Produces:
  - `pub enum Selection { Found(String), None, Missing(String) }` (`Debug`, `PartialEq`, `Eq`)
  - `pub fn select(root: &str, explicit: Option<&str>, is_file: &dyn Fn(&str) -> bool) -> Selection`
  - `pub fn missing_message(path: &str) -> String`

- [x] **Step 1: Write the failing tests**

Create `src/project_config.rs` with the tests only:

```rust
//! `lib/helpers/project_config.sh`: which `agent_sync.yaml` a project uses.

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(files: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |path: &str| files.contains(&path)
    }

    #[test]
    fn without_an_explicit_path_the_dot_ai_config_wins_then_the_root_one() {
        assert_eq!(select("/q", None, &probe(&[])), Selection::None);
        assert_eq!(
            select("/q", None, &probe(&["/q/agent_sync.yaml"])),
            Selection::Found("/q/agent_sync.yaml".into())
        );
        assert_eq!(
            select(
                "/q",
                None,
                &probe(&["/q/agent_sync.yaml", "/q/.ai/agent_sync.yaml"])
            ),
            Selection::Found("/q/.ai/agent_sync.yaml".into())
        );
    }

    #[test]
    fn an_explicit_path_is_relative_to_the_root_unless_absolute() {
        let files = probe(&["/q/config/a.yaml", "/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some("config/a.yaml"), &files),
            Selection::Found("/q/config/a.yaml".into())
        );
        assert_eq!(
            select("/q", Some("/q/config/a.yaml"), &files),
            Selection::Found("/q/config/a.yaml".into())
        );
    }

    #[test]
    fn a_missing_explicit_path_never_falls_back() {
        let files = probe(&["/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some("config/none.yaml"), &files),
            Selection::Missing("/q/config/none.yaml".into())
        );
        assert_eq!(
            select("/q", Some("dir.yaml"), &files),
            Selection::Missing("/q/dir.yaml".into())
        );
    }

    #[test]
    fn an_empty_explicit_path_is_unset() {
        let files = probe(&["/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some(""), &files),
            Selection::Found("/q/.ai/agent_sync.yaml".into())
        );
    }

    #[test]
    fn the_missing_message_is_the_bash_sentence() {
        assert_eq!(
            missing_message("/q/missing.yaml"),
            "AGENTSYNC_CONFIG_PATH is set but file not found: /q/missing.yaml"
        );
    }
}
```

In `src/lib.rs`, add `pub mod project_config;` in alphabetical position among the `pub mod` lines.

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test project_config 2>&1 | tail -5`
Expected: compile errors `cannot find type `Selection` in this scope` and `cannot find function `select``.

- [x] **Step 3: Write the implementation**

Insert above `#[cfg(test)]` in `src/project_config.rs`:

```rust
/// What `project_config_path_r` answered.
#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    /// The config file to read.
    Found(String),
    /// No explicit path, and neither `.ai/agent_sync.yaml` nor `agent_sync.yaml`.
    None,
    /// `AGENTSYNC_CONFIG_PATH` names this path, which is not a regular file.
    Missing(String),
}

/// `project_config_path_r`: an explicit path, relative to `root` unless
/// absolute, is authoritative and never falls back; otherwise
/// `.ai/agent_sync.yaml`, then `agent_sync.yaml`. `is_file` answers `[[ -f ]]`.
pub fn select(root: &str, explicit: Option<&str>, is_file: &dyn Fn(&str) -> bool) -> Selection {
    if let Some(raw) = explicit.filter(|raw| !raw.is_empty()) {
        let path = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("{root}/{raw}")
        };
        return if is_file(&path) {
            Selection::Found(path)
        } else {
            Selection::Missing(path)
        };
    }
    [
        format!("{root}/.ai/agent_sync.yaml"),
        format!("{root}/agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| is_file(path))
    .map_or(Selection::None, Selection::Found)
}

/// The sentence every command prints for [`Selection::Missing`].
pub fn missing_message(path: &str) -> String {
    format!("AGENTSYNC_CONFIG_PATH is set but file not found: {path}")
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test project_config 2>&1 | grep 'test result'`
Expected: `test result: ok. 5 passed` on the unit line.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/project_config.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "feat(native): select agent_sync.yaml like project_config_path_r"
```

Expected: both lint commands exit 0.

---

### Task 2: The version pin policy as `src/version.rs`

**Files:**
- Create: `src/version.rs`
- Modify: `src/lib.rs` (module list)

**Interfaces:**
- Consumes: `crate::yaml_subset::value(text: &str, key_path: &str) -> String`.
- Produces:
  - `pub enum Mode { Warn, Strict }` (`Clone`, `Copy`, `Debug`, `Default` = `Warn`, `PartialEq`, `Eq`)
  - `pub fn mode(config: &str) -> Result<Mode, String>` — `Err` carries the unknown value
  - `pub fn mismatch_error(pinned: &str, engine: &str, committed: bool) -> String`
  - `pub fn hint(pinned: &str, engine: &str) -> [String; 2]`

- [x] **Step 1: Write the failing tests**

Create `src/version.rs`:

```rust
//! `lib/helpers/version.sh`: the `version_pin` policy and its messages.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nested_mode_and_the_scalar_shorthand_both_read() {
        assert_eq!(mode("version_pin:\n  mode: strict\n"), Ok(Mode::Strict));
        assert_eq!(mode("version_pin: strict\n"), Ok(Mode::Strict));
        assert_eq!(
            mode("version_pin:\n  mode: \"strict\" # keep\n"),
            Ok(Mode::Strict)
        );
    }

    #[test]
    fn no_value_is_warn() {
        assert_eq!(mode("agentsync_version: \"1\"\n"), Ok(Mode::Warn));
        assert_eq!(mode("version_pin:\n  mode:\n"), Ok(Mode::Warn));
    }

    #[test]
    fn a_scalar_before_the_mapping_answers_first_like_bash_does() {
        // Known quirk 13.
        assert_eq!(
            mode("version_pin: warn\nversion_pin:\n  mode: strict\n"),
            Ok(Mode::Warn)
        );
    }

    #[test]
    fn an_unknown_value_is_returned_as_written() {
        assert_eq!(
            mode("version_pin:\n  mode: STRICT\n"),
            Err("STRICT".to_string())
        );
    }

    #[test]
    fn the_messages_are_the_bash_sentences() {
        assert_eq!(
            mismatch_error("0.1.0", "0.36.0", false),
            "This project pins agentsync 0.1.0 but you are running 0.36.0 — version_pin.mode 'strict' requires local outputs to use the pinned version."
        );
        assert_eq!(
            mismatch_error("0.1.0", "0.36.0", true),
            "This project pins agentsync 0.1.0 but you are running 0.36.0 — committed outputs must come from one version everywhere."
        );
        assert_eq!(
            hint("0.1.0", "0.36.0"),
            [
                "  • Match the pin:  agentsync update 0.1.0".to_string(),
                "  • Or move it:     agentsync upgrade-config   (re-pins to 0.36.0; re-sync and commit the outputs)".to_string(),
            ]
        );
    }
}
```

In `src/lib.rs`, add `pub mod version;` in alphabetical position.

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test version:: 2>&1 | tail -5`
Expected: compile errors for `mode`, `Mode`, `mismatch_error`, and `hint`.

- [x] **Step 3: Write the implementation**

Insert above `#[cfg(test)]` in `src/version.rs`:

```rust
use crate::yaml_subset;

/// `version_pin.mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Warn,
    Strict,
}

/// `version_pin_mode`: `version_pin.mode`, else the scalar `version_pin`, with
/// every `"` removed; no value is `warn`. An unknown value is the `Err`.
pub fn mode(config: &str) -> Result<Mode, String> {
    let mut value = yaml_subset::value(config, "version_pin.mode").replace('"', "");
    if value.is_empty() {
        value = yaml_subset::value(config, "version_pin").replace('"', "");
    }
    match value.as_str() {
        "" | "warn" => Ok(Mode::Warn),
        "strict" => Ok(Mode::Strict),
        _ => Err(value),
    }
}

/// `version_pin_mismatch_error` for outputs that are `committed` or not.
pub fn mismatch_error(pinned: &str, engine: &str, committed: bool) -> String {
    if committed {
        format!(
            "This project pins agentsync {pinned} but you are running {engine} — committed outputs must come from one version everywhere."
        )
    } else {
        format!(
            "This project pins agentsync {pinned} but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version."
        )
    }
}

/// `version_pin_mismatch_hint`.
pub fn hint(pinned: &str, engine: &str) -> [String; 2] {
    [
        format!("  • Match the pin:  agentsync update {pinned}"),
        format!(
            "  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)"
        ),
    ]
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test version:: 2>&1 | grep 'test result'`
Expected: `test result: ok. 5 passed` on the unit line. If `a_scalar_before_the_mapping_answers_first_like_bash_does` fails, `yaml_subset::value` diverges from `parse_yaml_value`: confirm with `bash -c 'source lib/helpers/yaml.sh; printf "version_pin: warn\nversion_pin:\n  mode: strict\n" > /tmp/v.yaml; parse_yaml_value /tmp/v.yaml version_pin.mode'` (prints an empty line) and fix the reader, not the test.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/version.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "feat(native): read version_pin.mode like version.sh"
```

---

### Task 3: `sync` Fails Closed on Config Selection and Enforces the Pin Mode

**Files:**
- Modify: `src/render.rs` (`Run`, `render`, `load_run_config`, `load_tools`, `check_version_pin`; new `user_tool_slugs`, `refuse_configless_cleanup`; tests)
- Modify: `src/cli/sync.rs` (`sync`)

**Interfaces:**
- Consumes: `project_config::{select, Selection, missing_message}` (Task 1); `version::{mode, Mode, mismatch_error, hint}` (Task 2); `Workspace::glob(&self, dir: &str) -> Vec<String>`; `Workspace::is_file(&self, path: &str) -> bool`.
- Produces:
  - `pub struct Run { …, pub version_pin: version::Mode, … }`
  - `pub fn refuse_configless_cleanup(s: &mut Session, run: &Run) -> Step`
  - `render(s, env)` now runs `prepare`, `refuse_configless_cleanup`, `check_version_pin`, then the stages it ran before.

- [x] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/render.rs`:

```rust
    fn with_config(path: &str) -> Env {
        Env {
            config_path: Some(path.to_string()),
            ..Env::default()
        }
    }

    #[test]
    fn a_missing_explicit_config_stops_without_falling_back() {
        let mut s = project();
        file(&mut s, "/proj/.ai/agent_sync.yaml", "tools:\n  enabled: [claude]\n");
        assert_eq!(render(&mut s, &with_config("missing.yaml")), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] AGENTSYNC_CONFIG_PATH is set but file not found: /proj/missing.yaml"]
        );
    }

    #[test]
    fn without_a_config_a_run_with_no_enabled_tool_is_refused() {
        let mut s = project();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'agentsync enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH."]
        );
    }

    #[test]
    fn without_a_config_a_tool_enabled_in_its_own_yaml_still_renders() {
        let mut s = project();
        file(&mut s, "/proj/.ai/src/tools/claude.yaml", "enabled: true\n");
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
    }

    #[test]
    fn a_strict_pin_stops_local_outputs_and_an_unknown_mode_stops_first() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "outputs: local\nagentsync_version: \"0.0.1\"\nversion_pin:\n  mode: strict\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        let engine = engine_version();
        assert_eq!(
            s.log.tail(3),
            [
                format!("[ERROR] This project pins agentsync 0.0.1 but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version.").as_str(),
                "  • Match the pin:  agentsync update 0.0.1",
                format!("  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)").as_str(),
            ]
        );

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "version_pin:\n  mode: refuse\noutputs: shared\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml — expected 'warn' or 'strict'"]
        );
    }
```

The last test's second config also sets `outputs: shared`: Bash reports the pin mode first, so only that line may appear.

The existing tests `claude_renders_agents_rules_commands_payloads_and_the_engine_skill` and others write a config before rendering; `a_project_without_agents_md_stops_with_status_one` renders without one and still expects the `Source agents file not found` lines, which `prepare` prints before the refusal is reached.

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test render::tests 2>&1 | grep -E '^test |test result'`
Expected: three new tests fail (`a_missing_explicit_config_stops_without_falling_back` sees a `[WARNING]` line and a continued render; the refusal and pin tests see `Ok(())`); `without_a_config_a_tool_enabled_in_its_own_yaml_still_renders` already passes and guards the refusal's exception; every other `render::tests` test passes.

- [x] **Step 3: Write the implementation**

In `src/render.rs`, extend the `use crate::{…}` list with `project_config` and `version`.

Add the field to `Run`, after `outputs`:

```rust
    pub version_pin: version::Mode,
```

Replace the selection at the top of `load_run_config` (from `let root = s.paths.root.clone();` through the `let config = match &config_path {` block's opening) with:

```rust
    let root = s.paths.root.clone();
    let chosen = project_config::select(&root, env.config_path.as_deref(), &|path: &str| {
        s.ws.is_file(path)
    });
    let config_path = match chosen {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            s.log.error(&project_config::missing_message(&path));
            return Err(Stop(1));
        }
    };
    let config = match &config_path {
```

Add `let mut version_pin = version::Mode::Warn;` next to `let mut outputs = "local";`, and make this the first statement inside `if let (Some(text), Some(path)) = (&config, &config_path) {`:

```rust
        version_pin = match version::mode(text) {
            Ok(mode) => mode,
            Err(value) => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown version_pin.mode '{value}' in {shown} — expected 'warn' or 'strict'"
                ));
                return Err(Stop(1));
            }
        };
```

Add `version_pin,` after `outputs,` in the `Ok(Run { … })` literal.

Replace `render`'s body:

```rust
pub fn render(s: &mut Session, env: &Env) -> Step {
    let mut run = prepare(s, env, Selection::default())?;
    refuse_configless_cleanup(s, &run)?;
    check_version_pin(s, &run)?;
    banner(s);
    setup_overlays(s, &mut run, false)?;
    build_catalog(s, &mut run);
    run_passes(s, &mut run)
}
```

Add after `resolve_sources`:

```rust
/// `_refuse_configless_cleanup_or_exit`: without a project config, a write run
/// whose tools are all disabled would only remove every tool's outputs.
pub fn refuse_configless_cleanup(s: &mut Session, run: &Run) -> Step {
    if run.config.is_some() || s.dry_run {
        return Ok(());
    }
    let legacy_enabled = user_tool_slugs(s)
        .iter()
        .any(|slug| load_tool(s, slug).user_value("enabled") == "true");
    if legacy_enabled {
        return Ok(());
    }
    s.log.error(
        "No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'agentsync enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH.",
    );
    Err(Stop(1))
}
```

Replace the loop in `load_tools` and add the helper it now shares with the refusal:

```rust
/// `list_all_tools`, plus the enabled and profile-tool sets `warm_*_cache` build.
fn load_tools(s: &mut Session, run: &mut Run) {
    let mut all: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    if let Some(text) = &run.config {
        run.enabled.extend(yaml_subset::list(text, "tools.enabled"));
        run.profile_tools.extend(profiles::all_tools(text));
    }
    for slug in user_tool_slugs(s) {
        if load_tool(s, &slug).user_value("enabled") == "true" {
            run.enabled.insert(slug.clone());
        }
        all.insert(slug);
    }
    run.tools = all.into_iter().collect();
}

/// The `.ai/src/tools/<slug>.yaml` overrides, `_`-prefixed templates skipped.
fn user_tool_slugs(s: &Session) -> Vec<String> {
    let tools_dir = format!("{}/.ai/src/tools", s.paths.root);
    s.ws.glob(&tools_dir)
        .into_iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".yaml")?;
            let listed = !stem.starts_with('_') && s.ws.is_file(&format!("{tools_dir}/{name}"));
            listed.then(|| stem.to_string())
        })
        .collect()
}
```

Replace `check_version_pin`'s body from `let hint = [` to the end:

```rust
    let hint = version::hint(&pinned, engine);
    let committed = run.outputs == "committed";
    if committed || run.version_pin == version::Mode::Strict {
        s.log
            .error(&version::mismatch_error(&pinned, engine, committed));
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
```

In `src/cli/sync.rs`, inside `sync`, insert between the `--if-stale` early return and `render::check_version_pin(s, &run)?;`:

```rust
    render::refuse_configless_cleanup(s, &run)?;
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/config_safety.bats | grep '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/version_pin.bats | grep '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/sync.bats | grep -c '^not ok'
```

Expected: `161 passed` (unit: 147 + 5 + 5 + 4) and `11 passed` (integration); in `config_safety.bats` exactly these native failures remain:

```text
not ok 4 check hands a relative explicit config outside .ai to its isolated sync
not ok 7 read-only commands reject an invalid explicit config path instead of using the local config
```

`0` in `version_pin.bats`; and `0` in `sync.bats`. The `check` cases in both files pass here only loosely: the native `check` now prints the right `[ERROR]` inside its `Sync script failed during check` tail and exits 1, which their `*"…"*` matches accept. The byte-exact `❌ …` before the banner is Task 4's, and Task 6's parity fixtures hold it.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/render.rs src/cli/sync.rs docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "feat(native): fail closed on config selection and enforce version_pin.mode"
```

---

### Task 4: `check` Selects the Config Up Front and Applies the Same Pin Rule

**Files:**
- Modify: `src/cli/check.rs` (`check`, `version_pin_mismatch`, `seed_workspace`, `merge_shared_parent`; remove `project_config`; tests)

**Interfaces:**
- Consumes: `project_config::{select, Selection, missing_message}`; `version::{mode, Mode, mismatch_error, hint}`; `render::render` from Task 3.
- Produces:
  - `fn read_config(path: &str) -> Result<String, Error>`
  - `fn version_pin_mismatch(root: &str, path: Option<&str>, config: Option<&str>) -> Option<Vec<String>>`
  - `fn seed_workspace(root: &str, manifest: &[String], selected: Option<&str>) -> Result<Workspace, String>` (a local `config` already names the root `agent_sync.yaml` there)
  - `fn merge_shared_parent(ws: &mut Workspace, root: &str, config: Option<&str>) -> Result<(), Error>`

- [x] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/cli/check.rs`:

```rust
    fn with_config(path: &str) -> Env {
        Env {
            config_path: Some(path.to_string()),
            ..Env::default()
        }
    }

    #[test]
    fn a_missing_explicit_config_fails_before_the_banner() {
        let (_dir, root) = project();
        let report = check(&root, &with_config("missing.yaml")).unwrap();
        assert_eq!(
            report,
            Report {
                stdout: String::new(),
                stderr: format!(
                    "❌ AGENTSYNC_CONFIG_PATH is set but file not found: {root}/missing.yaml\n"
                ),
                status: 1,
            }
        );
    }

    #[test]
    fn a_relative_explicit_config_outside_dot_ai_drives_the_render() {
        let (_dir, root) = project();
        std::fs::remove_file(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        write(
            Path::new(&root),
            "config/agentsync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let report = check(&root, &with_config("config/agentsync.yaml")).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
    }

    #[test]
    fn a_strict_pin_and_an_unknown_mode_fail_local_outputs_before_the_banner() {
        let (_dir, root) = project();
        let engine = engine_version();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "outputs: local\nagentsync_version: \"0.0.1\"\nversion_pin:\n  mode: strict\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!((report.status, report.stdout.as_str()), (1, ""));
        assert_eq!(
            report.stderr,
            format!("❌ This project pins agentsync 0.0.1 but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version.\n  • Match the pin:  agentsync update 0.0.1\n  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)\n")
        );

        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "version_pin:\n  mode: refuse\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(
            report.stderr,
            "❌ Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml — expected 'warn' or 'strict'\n"
        );
        assert_eq!(report.status, 1);
    }

    #[test]
    fn gitignore_update_false_without_outputs_counts_as_committed() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "gitignore:\n  update: false\nagentsync_version: \"0.0.1\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stderr.starts_with(
            "❌ This project pins agentsync 0.0.1 but you are running "
        ));
        assert!(report.stderr.contains(
            "— committed outputs must come from one version everywhere.\n"
        ));
    }
```

In the existing test `manifest_outputs_that_match_the_render_are_in_sync`, change `seed_workspace(&root, &[]).unwrap()` to `seed_workspace(&root, &[], Some(&format!("{root}/.ai/agent_sync.yaml"))).unwrap()`.

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test cli::check 2>&1 | grep -E '^test |test result'`
Expected: the four new tests fail — the missing config renders on, the relative config is not read, and the strict and `gitignore.update` cases pass the pin — while the other six pass. Make the three-argument `seed_workspace` edit in the existing test together with Step 3, since it does not compile before.

- [x] **Step 3: Write the implementation**

In `src/cli/check.rs`, change the `use crate::{…}` line to:

```rust
use crate::{Error, engine_version, overlay, project_config, version, yaml_subset};
```

Replace the start of `check` up to `report.out("Checking AgentSync configuration synchronization...");`:

```rust
pub fn check(root: &str, env: &Env) -> Result<Report, Error> {
    let mut report = Report::default();
    let is_file = |path: &str| Path::new(path).is_file();
    let config_path = match project_config::select(root, env.config_path.as_deref(), &is_file) {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            report.err(&format!("❌ {}", project_config::missing_message(&path)));
            report.status = 1;
            return Ok(report);
        }
    };
    let config = config_path.as_deref().map(read_config).transpose()?;
    if let Some(message) = version_pin_mismatch(root, config_path.as_deref(), config.as_deref()) {
        for line in message {
            report.err(&line);
        }
        report.status = 1;
        return Ok(report);
    }
    report.out("Checking AgentSync configuration synchronization...");
```

Change the two calls below it:

```rust
    let ws = match seed_workspace(root, &manifest, config_path.as_deref()) {
```

```rust
    merge_shared_parent(&mut session.ws, root, config.as_deref())?;
```

Replace `project_config` and `version_pin_mismatch` with:

```rust
fn read_config(path: &str) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `_check_version_pin`: an unknown mode, or a mismatch with committed outputs
/// or `version_pin.mode: strict`, fails before the banner.
fn version_pin_mismatch(root: &str, path: Option<&str>, config: Option<&str>) -> Option<Vec<String>> {
    let (Some(path), Some(config)) = (path, config) else {
        return None;
    };
    let mode = match version::mode(config) {
        Ok(mode) => mode,
        Err(value) => {
            let shown = path.strip_prefix(&format!("{root}/")).unwrap_or(path);
            return Some(vec![format!(
                "❌ Unknown version_pin.mode '{value}' in {shown} — expected 'warn' or 'strict'"
            )]);
        }
    };
    let mut outputs = yaml_subset::value(config, "outputs").replace('"', "");
    if outputs.is_empty() && yaml_subset::value(config, "gitignore.update") == "false" {
        outputs = "committed".to_string();
    }
    let committed = outputs == "committed";
    if !committed && mode != version::Mode::Strict {
        return None;
    }
    let pinned = yaml_subset::value(config, "agentsync_version").replace('"', "");
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return None;
    }
    let [first, second] = version::hint(&pinned, engine);
    Some(vec![
        format!("❌ {}", version::mismatch_error(&pinned, engine, committed)),
        first,
        second,
    ])
}
```

Change `seed_workspace`'s signature and add the seeding of a config outside `.ai/` before `Ok(ws)`:

```rust
fn seed_workspace(root: &str, manifest: &[String], selected: Option<&str>) -> Result<Workspace, String> {
```

```rust
    if let Some(path) = selected
        && !path.starts_with(&format!("{root}/.ai/"))
        && path != format!("{root}/agent_sync.yaml")
    {
        ws.seed_from_disk(path, Path::new(path), &is_git)
            .map_err(|e| e.to_string())?;
    }
    Ok(ws)
```

Replace `merge_shared_parent`:

```rust
/// Merge the `shared:` parent into the workspace, as the isolated sync of
/// `lib/check.sh` inherits it.
fn merge_shared_parent(ws: &mut Workspace, root: &str, config: Option<&str>) -> Result<(), Error> {
    let Some(config) = config else {
        return Ok(());
    };
    if let Some(parent) = overlay::shared_parent_src(config, root) {
        let inherit = yaml_subset::value(config, "shared.inherit");
        overlay::merge_shared_parent(ws, &parent, &overlay::inherit_categories(&inherit))?;
    }
    Ok(())
}
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/config_safety.bats | grep '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/version_pin.bats | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/check.bats | grep -c '^not ok'
```

Expected: `165 passed` (unit) and `11 passed` (integration); only `not ok 7 read-only commands reject an invalid explicit config path instead of using the local config` in `config_safety.bats`; `0` in `version_pin.bats`; `0` in `check.bats`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/check.rs docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "feat(native): select the config and apply version_pin.mode in check"
```

---

### Task 5: `list` Refuses a Missing Explicit Config

**Files:**
- Modify: `src/error.rs` (new variant)
- Modify: `src/project.rs` (`discover`, `at`; new `select`; test)

**Interfaces:**
- Consumes: `project_config::{select, Selection}`; `paths::logical_root(env_root: Option<&str>, cwd: &Path, pwd: Option<&str>) -> String`.
- Produces:
  - `Error::ConfigPathNotFound(PathBuf)` with `Display` `AGENTSYNC_CONFIG_PATH is set but file not found: <path>`
  - `pub fn Project::select(root: impl Into<PathBuf>, explicit: Option<&str>) -> Result<Project, Error>`
  - `Project::discover` honours `AGENTSYNC_CONFIG_PATH` and spells the root as `lib/helpers/list.sh` does (`cd "$project_dir" && pwd`).

- [x] **Step 1: Write the failing test**

Append inside `mod tests` in `src/project.rs`:

```rust
    #[test]
    fn an_explicit_config_is_authoritative_and_a_missing_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/agent_sync.yaml", "tools:\n  enabled: [claude]\n");
        write(dir.path(), "config/a.yaml", "tools:\n  enabled: [zed]\n");

        let project = Project::select(dir.path(), Some("config/a.yaml")).unwrap();
        assert_eq!(project.configured_enabled_tools().unwrap(), ["zed"]);

        let err = Project::select(dir.path(), Some("missing.yaml")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "AGENTSYNC_CONFIG_PATH is set but file not found: {}/missing.yaml",
                dir.path().display()
            )
        );
    }
```

- [x] **Step 2: Run the test, confirm it fails**

Run: `cargo test project::tests 2>&1 | tail -5`
Expected: `no function or associated item named `select` found for struct `Project``.

- [x] **Step 3: Write the implementation**

In `src/error.rs`, add after `ProjectRootNotFound`:

```rust
    #[error("AGENTSYNC_CONFIG_PATH is set but file not found: {}", .0.display())]
    ConfigPathNotFound(PathBuf),
```

In `src/project.rs`, change the `use` lines to:

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::project_config::{self, Selection};
use crate::{Error, paths, yaml_subset};
```

Replace `discover` and `at`:

```rust
    /// `_list_prepare_context`: `AGENTSYNC_REPO_ROOT` when set, else the
    /// working directory, with `AGENTSYNC_CONFIG_PATH` authoritative.
    pub fn discover() -> Result<Self, Error> {
        let env_root = std::env::var("AGENTSYNC_REPO_ROOT")
            .ok()
            .filter(|root| !root.is_empty());
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let pwd = std::env::var("PWD").ok();
        let root = paths::logical_root(env_root.as_deref(), &cwd, pwd.as_deref());
        let explicit = std::env::var("AGENTSYNC_CONFIG_PATH").ok();
        Self::select(root, explicit.as_deref())
    }

    /// Config is `.ai/agent_sync.yaml`, falling back to a root-level `agent_sync.yaml`.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::select(root, None)
    }

    /// `tool_resolver_select_project_config`: an explicit path is authoritative.
    pub fn select(root: impl Into<PathBuf>, explicit: Option<&str>) -> Result<Self, Error> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::ProjectRootNotFound(root));
        }
        let shown = root.to_string_lossy().into_owned();
        let config_path = match project_config::select(&shown, explicit, &|path: &str| {
            Path::new(path).is_file()
        }) {
            Selection::Found(path) => Some(PathBuf::from(path)),
            Selection::None => None,
            Selection::Missing(path) => return Err(Error::ConfigPathNotFound(PathBuf::from(path))),
        };
        Ok(Self { root, config_path })
    }
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result'
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/config_safety.bats | grep -c '^not ok'
AGENTSYNC_NATIVE=1 bats --tap tests/list.bats | grep -c '^not ok'
```

Expected: `166 passed` (unit) and `11 passed` (integration); `0` in `config_safety.bats`; `0` in `list.bats`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/error.rs src/project.rs docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "feat(native): refuse a missing AGENTSYNC_CONFIG_PATH in list"
```

---

### Task 6: Parity Fixtures, Spec, and Module Map

**Files:**
- Modify: `tests/native_parity.bats` (new fixtures after `parity: check without a .ai directory` and after the last `sync` fixture)
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` (quirk 13; retire the Phase 2 `shared:` deviation)
- Modify: `.ai/src/skills/native-port/references/module-map.md` (rows for `project_config.sh` and `version.sh`)
- Modify: `.ai/.sync-manifest` (regenerated)

**Interfaces:**
- Consumes: `assert_parity`, `assert_tree_parity`, `_bash_sync`, `enable_tools` in `tests/native_parity.bats` and `tests/test_helper.bash`; the binary from Tasks 3 to 5.
- Produces: fixtures that fail when either engine's config selection or pin policy drifts.

- [x] **Step 1: Write the fixtures**

After `@test "parity: check without a .ai directory"` in `tests/native_parity.bats`:

```bash
@test "parity: config selection and version_pin.mode in check and list" {
    enable_tools claude
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_parity check
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_parity list
    mkdir -p config
    cp .ai/agent_sync.yaml config/agentsync.yaml
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml _bash_sync
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml assert_parity check
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml assert_parity list
    printf 'tools:\n  enabled: [claude]\noutputs: local\nagentsync_version: "0.0.1"\nversion_pin:\n  mode: strict\n' > .ai/agent_sync.yaml
    assert_parity check
    printf 'version_pin: nope\n' > .ai/agent_sync.yaml
    assert_parity check
    printf 'gitignore:\n  update: false\nagentsync_version: "0.0.1"\n' > .ai/agent_sync.yaml
    assert_parity check
}
```

After the last `@test "parity: sync …"` fixture (before the `rollback` ones):

```bash
@test "parity: sync fails closed on config selection and enforces version_pin.mode" {
    enable_tools claude
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity sync
    printf 'tools:\n  enabled: [claude]\noutputs: local\nagentsync_version: "0.0.1"\nversion_pin: strict\n' > .ai/agent_sync.yaml
    assert_tree_parity sync
    printf 'tools:\n  enabled: [claude]\nversion_pin:\n  mode: refuse\n' > .ai/agent_sync.yaml
    assert_tree_parity sync
    rm .ai/agent_sync.yaml
    assert_tree_parity sync
    assert_tree_parity sync --dry-run
    mkdir -p .ai/src/tools
    printf 'enabled: true\n' > .ai/src/tools/claude.yaml
    assert_tree_parity sync
}
```

- [x] **Step 2: Run the fixtures in both modes**

```bash
cargo build --release
bats --tap -f 'config selection' tests/native_parity.bats
AGENTSYNC_NATIVE=0 bats --tap -f 'config selection' tests/native_parity.bats
```

Expected: both runs print `ok` for the two new fixtures. A diff here is a port bug: fix the Rust, not the fixture.

- [x] **Step 3: Prove the fixtures bite**

Change `"— expected 'warn' or 'strict'"` to `"— expected 'warn' or 'strict'!"` in `src/render.rs`, then run `cargo build --release && bats --tap -f 'sync fails closed' tests/native_parity.bats`. Expected: `not ok` with a diff naming `expected 'warn' or 'strict'!`. Revert the change and rebuild.

- [x] **Step 4: Update the spec and the module map**

In `docs/specs/2026-09-12-rust-migration-design.md`, append to "Known quirks to reproduce now and fix after cutover":

```markdown
13. `version_pin: warn` followed later by a `version_pin:` mapping with
    `mode: strict` reads as `warn`: the reader answers the first `version_pin`
    key, so the nested lookup is empty and the scalar wins (`version.sh`,
    `version_pin_mode`).
```

Delete the accepted deviation that begins `- Phase 2: \`check\` reads \`agent_sync.yaml\` with its \`shared:\` block in place;` (three lines): since `8677ed9` `lib/check.sh` no longer strips the block.

In `.ai/src/skills/native-port/references/module-map.md`, replace the Tier 1 line for `lib/helpers/version.sh` with:

```text
lib/helpers/version.sh           → src/version.rs          version_pin mode, mismatch error and hint; engine_version stays in src/lib.rs
lib/helpers/project_config.sh    → src/project_config.rs   project_config_path_r over an is_file probe; shared by sync, check, list
```

- [x] **Step 5: Verify the family**

Run the bats files one at a time.

```bash
cargo test 2>&1 | grep 'test result'
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in config_safety version_pin check sync list outputs_mode team_workflow workspace native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force
```

Expected: `166 passed` and `11 passed`; clippy, fmt, and ShellCheck exit 0; every line `bash=0 native=0` except `native_parity bash=1 native=1` (the rollback usage fixture, family 4); the repository sync ends with `[DONE] Synced 2/13 tools (11 skipped)`.

- [x] **Step 6: Commit**

```bash
git add tests/native_parity.bats docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md
git commit -m "test(native): diff Bash against native config selection and version_pin.mode"
```

---

## Completion

The family is closed when every box above is ticked, `config_safety.bats` and `version_pin.bats` are green under `AGENTSYNC_NATIVE=1`, the two new parity fixtures pass in both modes, and a `## Completion receipt` maps each Global Constraint to the file that satisfies it with fresh command output. The next family is backup retention, planned in its own file.

## Run log

### 2026-09-14 — Phase 3b family 1 planned
- Commits: `07cf85f` Merge branch 'main' into feat/native-engine-phase-1; `8677ed9` fix(check): let the isolated sync inherit shared sources itself; this plan with the spec's Phase 3b section.
- Verified: after the merge, `cargo test` 147 + 11 passed, clippy and fmt clean, release build ok; `AGENTSYNC_NATIVE=0 bats --jobs 6 --tap tests/` 868 tests, 1 failure (`parity: rollback plans, restores, and refuses like Bash`, family 4); `AGENTSYNC_NATIVE=1` failures by file: `config_safety` 5 of 7, `version_pin` 5 of 13, `backup_retention` 10, `source_overrides` 18 of 28, `native_parity` 1; `rollback_preflight` pending when the plan was written. The native run stopped after `resource_resolver.bats`: two full-suite runs with `--jobs 6` and `--jobs 3` were killed by the system for low memory, so `rollback`, `shared`, `shell_init`, `simplify`, `sync_options`, `sync`, `team_workflow`, `tmp`, `update_snapshot`, `update`, and `workspace` have no native count after the merge yet; Task 0 records them.
- Plan amended: none.
- Next: Task 0 Step 1, after the review takes the four decisions.
- Blocker: none.

### 2026-09-14 — Tasks 0–6 done, family 1 served natively
- Commits: `f664ea1` select agent_sync.yaml like project_config_path_r; `e47afea` read version_pin.mode like version.sh; `add4969` fail closed on config selection and enforce version_pin.mode; `130f0d4` select the config and apply version_pin.mode in check; `521c17c` refuse a missing AGENTSYNC_CONFIG_PATH in list; the Task 6 commit with the parity fixtures, spec, and module map.
- Verified: Task 0 native baseline `check`, `list`, `sync`, `outputs_mode`, `team_workflow`, `workspace` all `0`. After Task 6: `cargo test` 166 + 11 passed; clippy, fmt, and ShellCheck exit 0; one file at a time in both modes, `config_safety`, `version_pin`, `check`, `sync`, `list`, `outputs_mode`, `team_workflow`, `workspace` `bash=0 native=0`, `native_parity` `bash=1 native=1` (only `parity: rollback plans, restores, and refuses like Bash`, family 4); the two new fixtures pass, and appending `!` to the unknown-mode message made `parity: sync fails closed …` fail with that diff before it was reverted; repository `sync --force` ended `[DONE] Synced 2/13 tools (11 skipped)`.
- Plan amended: Task 3 Step 2 expects three failing tests, not four (the legacy-enabled test guards the refusal's exception and passes throughout); Task 3 Step 3's local `selection` renamed `chosen`, since `load_run_config` already takes a `selection` parameter; Task 3 Step 4 expects only tests 4 and 7 in `config_safety.bats` and none in `version_pin.bats`, whose `check` cases pass loosely once the shared render fails closed; Task 3's commit subject shortened to 72 characters; Task 4's `seed_workspace` parameter renamed `selected`, since the function already binds a local `config`, and Step 2's expectation reworded to four failing tests without the compile error.
- Next: close family 1 with its completion receipt, then plan family 2 (backup retention).
- Blocker: none.
