# Rust Migration Phase 3b, Family 2: Backup Retention

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the native `sync` and `rollback` read `backup.retention: bounded | preserve`, validate it and the backup bounds before they write, and keep every existing snapshot and staging entry under `preserve`, exactly as release 0.36.0's `lib/helpers/backup.sh` does.

**Architecture:** `yaml_subset::found` answers what `YAML_VALUE_FOUND` tells Bash: a key present with an empty value is not a missing key. `backup::configure` is `backup_configure` after the config is selected; it returns a `Retention` that `create` passes to `sweep_stale_staging` and `prune` checks after validating the bounds. `sync` configures inside `render::load_run_config`, right after config selection and before the version pin mode, unless the run takes no backup, so the messages keep Bash's order; `rollback` configures after `--list`, before it looks up the snapshot. The test seam stays the CLI process boundary: `tests/backup_retention.bats` under `AGENTSYNC_NATIVE=1` and new parity fixtures; unit tests assert the values captured from `backup_configure` on 2026-09-14.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, section "Phase 3b", family 2. Previous plan: `docs/plans/2026-09-14-rust-migration-phase-3b-config-and-pin.md` (closed, receipt 2026-09-14).

## Global Constraints

- `.ai/src/` remains the source of truth. The native `sync` and `rollback` write only what Bash writes; under `preserve` they delete nothing that existed in `.ai/backups/` when they started.
- No binary ships to users; this family changes no Bash, and `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean after every task; no new dependency.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected message was captured from Bash 0.36.0 on 2026-09-14 by `scratchpad/phase3b/bash_reference_retention.sh`.
- Accepted deviations and quirks already in the spec still hold; this family adds none.
- Commits follow Conventional Commits, scope `native`, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time: full-suite runs exhausted memory on the development machine on 2026-09-14.

## Decisions for the review

Taken on 2026-09-14 by the maintainer's instruction to proceed without waiting: all three as recommended.

1. **`create` and `prune` take the retention.** **Recommended:** add a `retention: Retention` parameter to `backup::create` and `backup::prune` (and their `_at` twins) and pass `Retention::Bounded` in the existing tests, so no caller can forget the policy. Alternative: `create_with`/`prune_with` beside the old functions.
2. **Where `sync` validates.** **Recommended:** in `render::load_run_config`, gated by a new `render::Env::backup` that `main` fills only when `AGENTSYNC_INTERNAL_SKIP_BACKUP` is not `true`; `check` leaves it `None`. Alternative: validate in `cli::sync` after `prepare`, which reorders the messages when a config has both an invalid policy and an unknown pin mode.
3. **`yaml_subset::found`.** **Recommended:** `value` becomes `found(..).unwrap_or_default()`, so both share one walk. Alternative: a second walk that only reports presence.

## Module closure

```text
lib/helpers/yaml.sh               36-112    parse_yaml_value_r: YAML_VALUE_FOUND at both return points
lib/helpers/backup.sh             26-65     backup_configure, _backup_validate_limits
                                  336-356   _backup_sweep_stale_staging: preserve returns first
                                  684-700   backup_prune: validate, then preserve returns
                                  846-866   cmd_rollback: --list, then backup_configure
lib/sync.sh                       743-747   _load_run_config: backup_configure unless AGENTSYNC_INTERNAL_SKIP_BACKUP
```

---

### Task 0: Baseline

**Files:** none changed.

**Interfaces:** consumes branch `feat/native-engine-phase-1` at `04929db` or later.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/backup_retention.bats | grep '^not ok'
for f in backup rollback sync; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `04929db docs(native): close phase 3b family 1`; `166 passed` and `11 passed`; these native failures in `backup_retention.bats`:

```text
not ok 3 retention preserve sync keeps snapshots and every staging kind despite low limits
not ok 4 retention preserve sync with zero limits also keeps abandoned staging
not ok 5 retention invalid or empty values reject sync before any project mutation
not ok 6 retention malformed backup section rejects sync before changes
not ok 9 retention preserve failed sync restores outputs without pruning recovery
not ok 10 retention preserve rollback retains its policy when restoring a bounded config
not ok 11 retention invalid rollback config fails before safety snapshot or restore
not ok 12 retention uses the explicit config instead of a conflicting local policy
not ok 13 retention invalid numeric bounds reject sync before changes
not ok 15 retention invalid explicit config rejects init and rollback without fallback
```

and `0` for `backup`, `rollback`, and `sync`. The test numbers are the file's own; the run log of family 1 counted them in the full suite.

---

### Task 1: `yaml_subset::found`

**Files:**
- Modify: `src/yaml_subset.rs` (`value` → `found`; tests)

**Interfaces:**
- Produces: `pub fn found(text: &str, key_path: &str) -> Option<String>`; `value(text, key_path)` keeps its signature and returns `found(..).unwrap_or_default()`.

- [x] **Step 1: Write the failing test**

Append inside `mod tests` in `src/yaml_subset.rs`:

```rust
    #[test]
    fn found_tells_an_empty_value_from_a_missing_key() {
        let text = "backup:\n  retention:\n  other: \"\"\n  note: # nothing\nkeep: x\n";
        assert_eq!(found(text, "backup.retention"), Some(String::new()));
        assert_eq!(found(text, "backup.other"), Some(String::new()));
        assert_eq!(found(text, "backup.note"), Some(String::new()));
        assert_eq!(found(text, "backup.missing"), None);
        assert_eq!(found(text, "keep"), Some("x".to_string()));
        assert_eq!(found(text, "backup"), Some(String::new()));
        assert_eq!(found("backup: preserve\n", "backup.retention"), None);
    }
```

- [x] **Step 2: Run the test, confirm it fails**

Run: `cargo test yaml_subset::tests::found 2>&1 | grep -E '^error\['`
Expected: `error[E0425]: cannot find function `found` in this scope`.

- [x] **Step 3: Write the implementation**

Replace `pub fn value(text: &str, key_path: &str) -> String {` and its body's return points:

```rust
/// The scalar at a dotted key path (`targets.rules.dest`), or `""` when the
/// key is missing or empty. Nesting is decided by indentation and the first
/// occurrence of a key wins.
pub fn value(text: &str, key_path: &str) -> String {
    found(text, key_path).unwrap_or_default()
}

/// [`value`], telling a key present with an empty value (`Some("")`) from a
/// missing one (`None`), as `YAML_VALUE_FOUND` does.
pub fn found(text: &str, key_path: &str) -> Option<String> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut level = 0usize;
    let mut section_indent = 0usize;
    let mut in_section = false;

    for line in text.lines() {
        if is_blank_or_comment(line) {
            continue;
        }
        let (indent, stripped) = strip_indent(line);
        let Some((key, rest)) = split_key(stripped) else {
            continue;
        };
        if !in_section {
            if indent != 0 || key != keys[0] {
                continue;
            }
        } else {
            if indent <= section_indent {
                return None;
            }
            if key != keys[level] {
                continue;
            }
        }
        if level + 1 == keys.len() {
            return Some(normalize_scalar(rest));
        }
        in_section = true;
        section_indent = indent;
        level += 1;
    }
    None
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `167 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/yaml_subset.rs docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md
git commit -m "feat(native): tell an empty YAML value from a missing key"
```

---

### Task 2: `backup::Retention`, `configure`, and the Policy in `create` and `prune`

**Files:**
- Modify: `src/backup.rs` (new `Retention`, `configure`, `validate_limits`; `sweep_stale_staging`, `create`, `create_at`, `prune`, `prune_at`; tests)
- Modify: `src/cli/sync.rs`, `src/cli/rollback.rs` (pass `backup::Retention::Bounded` until Tasks 3 and 4)

**Interfaces:**
- Consumes: `yaml_subset::{value, found}` (Task 1).
- Produces:
  - `pub enum Retention { Bounded, Preserve }` (`Clone`, `Copy`, `Debug`, `Default` = `Bounded`, `PartialEq`, `Eq`)
  - `pub fn configure(config: Option<(&str, &str)>, limit: Option<&str>, max_age: Option<&str>) -> Result<Retention, Error>` — `config` is `(path, text)` of the selected `agent_sync.yaml`
  - `pub fn create(supplied_root: &str, operation: &str, targets: &[String], retention: Retention) -> Result<String, Error>`
  - `pub fn prune(supplied_root: &str, limit: Option<&str>, max_age: Option<&str>, retention: Retention) -> Result<(), Error>`

- [x] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/backup.rs`:

```rust
    #[test]
    fn configure_reads_the_policy_and_the_bounds_like_backup_configure() {
        let path = "/p/.ai/agent_sync.yaml";
        let with = |text: &str| configure(Some((path, text)), None, None);
        assert_eq!(configure(None, None, None).unwrap(), Retention::Bounded);
        assert_eq!(with("tools:\n  enabled: [claude]\n").unwrap(), Retention::Bounded);
        assert_eq!(with("backup:\n  retention: preserve\n").unwrap(), Retention::Preserve);
        assert_eq!(with("backup:\n  retention: bounded\n").unwrap(), Retention::Bounded);
        assert_eq!(
            with("backup:\n  retention: \"preserve\" # keep\n").unwrap(),
            Retention::Preserve
        );
        assert_eq!(with("backup:\n  other: 1\n").unwrap(), Retention::Bounded);
        let refused = |text: &str| with(text).unwrap_err().to_string();
        assert_eq!(
            refused("backup:\n  retention: typo\n"),
            "Invalid backup.retention 'typo' in /p/.ai/agent_sync.yaml; expected bounded or preserve"
        );
        for empty in ["backup:\n  retention:\n", "backup:\n  retention: \"\"\n", "backup:\n  retention: # nothing\n"] {
            assert_eq!(
                refused(empty),
                "Invalid backup.retention '<empty>' in /p/.ai/agent_sync.yaml; expected bounded or preserve"
            );
        }
        assert_eq!(
            refused("backup: preserve\n"),
            "backup must be a mapping with backup.retention: bounded or preserve in /p/.ai/agent_sync.yaml"
        );
        let bounds = |limit: &str, age: &str| {
            configure(None, Some(limit), Some(age)).map_err(|e| e.to_string())
        };
        assert_eq!(bounds("typo", "").unwrap_err(), "Backup limit must be a non-negative integer: typo");
        assert_eq!(bounds("", "-1").unwrap_err(), "Backup max age must be a non-negative integer: -1");
        assert_eq!(bounds("x", "y").unwrap_err(), "Backup limit must be a non-negative integer: x");
        assert_eq!(bounds("0", "0").unwrap(), Retention::Bounded);
    }

    #[test]
    fn preserve_keeps_old_snapshots_and_stale_staging() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.fake_snapshot("20200102T000000Z-sync-2");
        p.write(".ai/backups/.latest", "20200102T000000Z-sync-2\n");
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.abandoned/files")).unwrap();
        std::fs::File::open(p.path(".ai/backups/.tmp.sync.abandoned"))
            .unwrap()
            .set_modified(at(1_577_836_800))
            .unwrap();
        let now = at(1_789_323_442);
        create_at(&p.root, "sync", &[p.abs("AGENTS.md")], now, Retention::Preserve).unwrap();
        assert!(p.path(".ai/backups/.tmp.sync.abandoned").exists());
        prune_at(&p.root, Some("1"), Some("1"), now, Retention::Preserve).unwrap();
        assert!(p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        assert!(p.path(".ai/backups/20200102T000000Z-sync-2").exists());
        assert_eq!(
            prune_at(&p.root, Some("x"), None, now, Retention::Preserve)
                .unwrap_err()
                .to_string(),
            "Backup limit must be a non-negative integer: x"
        );
    }
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test backup::tests 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: errors for the missing `configure` and `Retention`, and for `create_at`/`prune_at` taking four arguments.

- [x] **Step 3: Write the implementation**

In `src/backup.rs`, extend the `use crate::{…}` line with `yaml_subset`, then add after `fn refuse`:

```rust
/// `backup.retention`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Retention {
    #[default]
    Bounded,
    Preserve,
}

/// `backup_configure` once the project config is selected: the retention
/// policy in `config` (`(path, text)`, when there is one), then the bounds.
pub fn configure(
    config: Option<(&str, &str)>,
    limit: Option<&str>,
    max_age: Option<&str>,
) -> Result<Retention, Error> {
    let mut retention = Retention::Bounded;
    if let Some((path, text)) = config {
        if !yaml_subset::value(text, "backup").is_empty() {
            return Err(refuse(format!(
                "backup must be a mapping with backup.retention: bounded or preserve in {path}"
            )));
        }
        if let Some(value) = yaml_subset::found(text, "backup.retention") {
            retention = match value.as_str() {
                "bounded" => Retention::Bounded,
                "preserve" => Retention::Preserve,
                other => {
                    let shown = if other.is_empty() { "<empty>" } else { other };
                    return Err(refuse(format!(
                        "Invalid backup.retention '{shown}' in {path}; expected bounded or preserve"
                    )));
                }
            };
        }
    }
    validate_limits(limit, max_age)?;
    Ok(retention)
}

/// `_backup_validate_limits` with `:-10` and `:-30` applied; the parsed bounds.
fn validate_limits(limit: Option<&str>, max_age: Option<&str>) -> Result<(u64, u64), Error> {
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
    Ok((
        limit.parse().unwrap_or(u64::MAX),
        max_age.parse().unwrap_or(u64::MAX),
    ))
}
```

`sweep_stale_staging` gains the policy:

```rust
fn sweep_stale_staging(store: &str, now: SystemTime, retention: Retention) {
    if retention == Retention::Preserve {
        return;
    }
```

`create` and `create_at` gain it and pass it on:

```rust
/// `backup_create`: the snapshot's path.
pub fn create(
    supplied_root: &str,
    operation: &str,
    targets: &[String],
    retention: Retention,
) -> Result<String, Error> {
    create_at(supplied_root, operation, targets, SystemTime::now(), retention)
}

fn create_at(
    supplied_root: &str,
    operation: &str,
    targets: &[String],
    now: SystemTime,
    retention: Retention,
) -> Result<String, Error> {
```

with `sweep_stale_staging(&store, now, retention);` inside. `prune` and `prune_at` gain it and use `validate_limits`:

```rust
/// `backup_prune`: a snapshot survives when it is among the newest `limit` and
/// at most `max_age` whole UTC days old; 0 disables a bound; the latest stays.
/// Under `preserve` the bounds are still validated and nothing is removed.
pub fn prune(
    supplied_root: &str,
    limit: Option<&str>,
    max_age: Option<&str>,
    retention: Retention,
) -> Result<(), Error> {
    prune_at(supplied_root, limit, max_age, SystemTime::now(), retention)
}

fn prune_at(
    supplied_root: &str,
    limit: Option<&str>,
    max_age: Option<&str>,
    now: SystemTime,
    retention: Retention,
) -> Result<(), Error> {
    let (limit, max_age) = validate_limits(limit, max_age)?;
    if retention == Retention::Preserve {
        return Ok(());
    }
```

replacing the old defaults, digit checks, and parses at the top of `prune_at`.

Existing callers pass `Retention::Bounded` for now: every `create(`, `create_at(`, and `prune_at(` call in `src/backup.rs`'s tests gains `, Retention::Bounded` as the last argument; `src/cli/sync.rs` becomes `backup::create(&root, "sync", &targets, backup::Retention::Bounded)` and its `backup::prune(` call gains `backup::Retention::Bounded`; `src/cli/rollback.rs` likewise for `backup::create(&root, "rollback", &current, …)`, its `backup::prune(`, and the test's `backup::create(&root, "sync", &targets, …)`.

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `169 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/backup.rs src/cli/sync.rs src/cli/rollback.rs docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md
git commit -m "feat(native): read backup.retention and keep recovery under preserve"
```

---

### Task 3: `sync` Validates and Honours the Policy

**Files:**
- Modify: `src/render.rs` (`Env`, `Run`, `load_run_config`; test)
- Modify: `src/cli/sync.rs` (`Transaction`, `prune`, `start_transaction`)
- Modify: `src/main.rs` (`sync_env`, `Command::Check`)

**Interfaces:**
- Consumes: `backup::{configure, Retention}` (Task 2).
- Produces:
  - `pub struct BackupBounds { pub limit: Option<String>, pub max_age: Option<String> }` in `render`
  - `render::Env { …, pub backup: Option<BackupBounds> }` — `None` skips `backup_configure`
  - `render::Run { …, pub retention: backup::Retention }`

- [x] **Step 1: Write the failing test**

Append inside `mod tests` in `src/render.rs`:

```rust
    #[test]
    fn an_invalid_backup_policy_stops_before_the_pin_mode_when_the_run_backs_up() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "backup:\n  retention: typo\nversion_pin:\n  mode: refuse\n",
        );
        let env = Env {
            backup: Some(BackupBounds::default()),
            ..Env::default()
        };
        assert_eq!(render(&mut s, &env), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["Error: Invalid backup.retention 'typo' in /proj/.ai/agent_sync.yaml; expected bounded or preserve"]
        );

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nbackup:\n  retention: typo\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
    }
```

- [x] **Step 2: Run the test, confirm it fails**

Run: `cargo test render::tests::an_invalid_backup 2>&1 | grep -E '^error\['`
Expected: `cannot find struct, variant or union type `BackupBounds`` and `struct `render::Env` has no field named `backup``.

- [x] **Step 3: Write the implementation**

In `src/render.rs`, add `backup` to the `use crate::{…}` list, then:

```rust
/// `AGENTSYNC_BACKUP_LIMIT` and `AGENTSYNC_BACKUP_MAX_AGE_DAYS` for a run that
/// takes a backup.
#[derive(Default)]
pub struct BackupBounds {
    pub limit: Option<String>,
    pub max_age: Option<String>,
}
```

Add to `Env`:

```rust
    /// The bounds `backup_configure` validates; `None` when the run takes no
    /// backup (`AGENTSYNC_INTERNAL_SKIP_BACKUP=true`, and `check`).
    pub backup: Option<BackupBounds>,
```

Add `pub retention: backup::Retention,` to `Run` after `version_pin`, and in `load_run_config`, directly after the `let config = match &config_path { … };` block:

```rust
    let mut retention = backup::Retention::Bounded;
    if let Some(bounds) = &env.backup {
        let selected = config_path.as_deref().zip(config.as_deref());
        match backup::configure(selected, bounds.limit.as_deref(), bounds.max_age.as_deref()) {
            Ok(policy) => retention = policy,
            Err(e) => {
                s.log.err(format!("Error: {e}"));
                return Err(Stop(1));
            }
        }
    }
```

and `retention,` in the `Ok(Run { … })` literal.

In `src/cli/sync.rs`, give `Transaction` the policy and use it everywhere a backup is created or pruned:

```rust
#[derive(Default)]
struct Transaction {
    backup: Option<String>,
    active: bool,
    retention: backup::Retention,
}
```

`fn prune(s: &mut Session, env: &Env, retention: backup::Retention)` passes `retention` to `backup::prune`; `Transaction::fail` calls `prune(s, env, self.retention)`; `sync` calls `prune(s, env, run.retention)`; `start_transaction` sets `tx.retention = run.retention;` before `backup::create(&root, "sync", &targets, run.retention)`.

In `src/main.rs`, `sync_env` fills the bounds unless the run skips its backup:

```rust
fn sync_env() -> cli::sync::Env {
    let skip_backup = var("AGENTSYNC_INTERNAL_SKIP_BACKUP").as_deref() == Some("true");
    cli::sync::Env {
        render: Env {
            config_path: var("AGENTSYNC_CONFIG_PATH"),
            skip_post_sync: var("AGENTSYNC_SKIP_POST_SYNC"),
            allow_post_sync: var("AGENTSYNC_ALLOW_POST_SYNC"),
            backup: (!skip_backup).then(|| agentsync::render::BackupBounds {
                limit: var("AGENTSYNC_BACKUP_LIMIT"),
                max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
            }),
        },
        skip_backup,
        backup_limit: var("AGENTSYNC_BACKUP_LIMIT"),
        backup_max_age: var("AGENTSYNC_BACKUP_MAX_AGE_DAYS"),
    }
}
```

and `Command::Check`'s `Env { … }` literal gains `backup: None,`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/backup_retention.bats | grep '^not ok'
printf 'sync native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/sync.bats | grep -c '^not ok')"
```

Expected: `170 passed` and `11 passed`; only the rollback cases remain in `backup_retention.bats`:

```text
not ok 10 retention preserve rollback retains its policy when restoring a bounded config
not ok 11 retention invalid rollback config fails before safety snapshot or restore
not ok 15 retention invalid explicit config rejects init and rollback without fallback
```

and `sync native=0`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/render.rs src/cli/sync.rs src/main.rs docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md
git commit -m "feat(native): validate and honour backup.retention in sync"
```

---

### Task 4: `rollback` Validates and Honours the Policy

**Files:**
- Modify: `src/cli/rollback.rs` (`Env`, `run`; test)
- Modify: `src/main.rs` (`Command::Rollback`)

**Interfaces:**
- Consumes: `backup::{configure, Retention}`; `project_config::{select, Selection, missing_message}`.
- Produces: `rollback::Env { pub config_path: Option<String>, pub backup_limit: Option<String>, pub backup_max_age: Option<String> }`.

- [x] **Step 1: Write the failing test**

Append inside `mod tests` in `src/cli/rollback.rs`:

```rust
    #[test]
    fn the_policy_is_checked_after_list_and_preserve_keeps_the_history() {
        let (dir, root) = project();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: typo\n",
        )
        .unwrap();
        let targets = [format!("{root}/CLAUDE.md")];
        backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        assert_eq!(rollback(&root, &["--list"], true).status, 0);
        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(
            refused.err,
            format!("Error: Invalid backup.retention 'typo' in {root}/.ai/agent_sync.yaml; expected bounded or preserve\n")
        );

        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: preserve\n",
        )
        .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            backup_limit: Some("1".into()),
            ..Env::default()
        };
        assert_eq!(run(&root, &["--yes".to_string()], &env, &mut |_| true, &mut out, &mut err), 0);
        assert_eq!(backup::list(&root).unwrap().len(), 2);

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            config_path: Some("missing.yaml".into()),
            ..Env::default()
        };
        assert_eq!(run(&root, &["--yes".to_string()], &env, &mut |_| true, &mut out, &mut err), 1);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("Error: AGENTSYNC_CONFIG_PATH is set but file not found: {root}/missing.yaml\n")
        );
    }
```

- [x] **Step 2: Run the test, confirm it fails**

Run: `cargo test cli::rollback 2>&1 | grep -E '^error\['`
Expected: `struct `cli::rollback::Env` has no field named `config_path``.

- [x] **Step 3: Write the implementation**

In `src/cli/rollback.rs`, change the `use crate::{…}` line to `use crate::{Error, backup, paths, project_config};` and `Env` to:

```rust
/// The environment `backup_configure` and `backup_prune` read.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}
```

Insert right after the `if list_only { … }` block:

```rust
    let is_file = |path: &str| std::path::Path::new(path).is_file();
    let config_path = match project_config::select(&root, env.config_path.as_deref(), &is_file) {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            let _ = writeln!(err, "Error: {}", project_config::missing_message(&path));
            return 1;
        }
    };
    let config = match config_path.as_deref().map(std::fs::read) {
        Some(Ok(bytes)) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Some(Err(e)) => return fail(err, Error::io(config_path.as_deref().unwrap_or_default(), e)),
        None => None,
    };
    let retention = match backup::configure(
        config_path.as_deref().zip(config.as_deref()),
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
    ) {
        Ok(retention) => retention,
        Err(e) => return fail(err, e),
    };
```

and pass `retention` to `backup::create(&root, "rollback", &current, retention)` and to `backup::prune(…, retention)`.

In `src/main.rs`, `Command::Rollback`'s `Env` gains `config_path: var("AGENTSYNC_CONFIG_PATH"),`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
printf 'backup_retention native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/backup_retention.bats | grep -c '^not ok')"
printf 'rollback native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/rollback.bats | grep -c '^not ok')"
```

Expected: `171 passed` and `11 passed`; `backup_retention native=0`; `rollback native=0`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/rollback.rs src/main.rs docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md
git commit -m "feat(native): validate and honour backup.retention in rollback"
```

---

### Task 5: Parity Fixtures and Module Map

**Files:**
- Modify: `tests/native_parity.bats` (fixture after `parity: sync fails closed …`)
- Modify: `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Write the fixture**

```bash
@test "parity: backup.retention in sync and rollback" {
    enable_tools claude
    mkdir -p .ai/backups/20200101T000000Z-sync-1/files .ai/backups/.tmp.sync.abandoned
    printf 'schema=1\noperation=sync\ncreated_at=20200101T000000Z\n' > .ai/backups/20200101T000000Z-sync-1/metadata
    : > .ai/backups/20200101T000000Z-sync-1/targets.tsv
    : > .ai/backups/20200101T000000Z-sync-1/.complete
    touch -t 202001010000 .ai/backups/.tmp.sync.abandoned
    printf 'backup:\n  retention: typo\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    AGENTSYNC_INTERNAL_SKIP_BACKUP=true assert_parity check
    sed 's/retention: typo/retention: preserve/' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 assert_tree_parity sync
    AGENTSYNC_BACKUP_LIMIT=typo assert_tree_parity sync
    _bash_sync
    AGENTSYNC_BACKUP_LIMIT=1 assert_tree_parity rollback --yes
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity rollback --yes
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity rollback --list
}
```

`assert_tree_parity` compares trees outside `.ai/backups`; the preserved history is covered by `backup_retention.bats`, whose Bash and native runs both assert it.

- [x] **Step 2: Run it and prove it bites**

```bash
cargo build --release
bats --tap -f 'backup.retention' tests/native_parity.bats
```

Expected: `ok`. Then change `"; expected bounded or preserve"` to `"; expected bounded or preserve!"` in `src/backup.rs`, rebuild, rerun: `not ok` with a diff naming `preserve!`; revert and rebuild.

- [x] **Step 3: Module map**

In `.ai/src/skills/native-port/references/module-map.md`, append to the Tier 2 line for `lib/helpers/manifest.sh`'s neighbour the backup entry, or, when the map lists `lib/helpers/backup.sh`, extend it with `; backup.retention (configure, Retention), validated before sync and rollback write`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 4: Verify the family**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in backup_retention backup rollback sync check config_safety native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `171 passed` and `11 passed`; lint exit 0; every line `bash=0 native=0` except `native_parity bash=1 native=1` (family 4's rollback usage fixture).

- [x] **Step 5: Commit**

```bash
git add tests/native_parity.bats .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md
git commit -m "test(native): diff Bash against native backup.retention"
```

---

## Completion

The family is closed when every box is ticked, `backup_retention.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixture passes in both modes, and a `## Completion receipt` records the fresh verification. The next family is sources outside the project.

## Run log

### 2026-09-14 — Phase 3b family 2 planned
- Commits: this plan.
- Verified: Bash reference values captured by `scratchpad/phase3b/bash_reference_retention.sh`; the native `backup_retention.bats` failures listed in Task 0 come from the post-merge run on 2026-09-14.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
