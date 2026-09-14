# Rust Migration Phase 3b, Family 4: The Rollback Witness

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the native `sync` and `rollback` seal `after.tsv` (`post-state-v2`) after they finish, and make the native `rollback` refuse a target changed since the backup's operation, naming the first changed path, restore it anyway under `--force`, and restore an unsealed snapshot with a warning, exactly as release 0.36.0's `lib/helpers/backup_state.sh`, `lib/helpers/backup.sh`, and `lib/sync.sh` do.

**Architecture:** A new `src/witness.rs` mirrors `backup_state.sh`: `print` walks the targets of `targets.tsv` into records, `seal` stages and renames `after.tsv`, `preflight` compares the live walk with the stored record and names the first difference. `backup.rs` opens the helpers the witness needs (`validate_rel`, `safe_target_path`, `create_unique`), refuses symlinked `.complete` and `targets.tsv` as 0.36.0 does, and gains `discard_safety`. `cli::sync` seals after the transaction ends and after a failed run is restored; `cli::rollback` takes `--force`, runs the preflight before the plan and again after the safety snapshot, and seals the safety snapshot. The seam stays the CLI process boundary: `tests/rollback_preflight.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures; disk-touching unit tests are `#[cfg(unix)]`.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 3b", family 4. Previous plan: `docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. A refused rollback writes nothing, and a refusal found after the safety snapshot removes that snapshot and restores `.latest`.
- No binary ships to users; this family changes no Bash under `lib/` or `bin/`, and `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations, including `after.tsv` itself.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. Hashes come from `manifest::sha256_hex`.
- Disk-touching unit tests are `#[cfg(unix)]`.
- `init` stays Bash; its seal is Bash's, and the native `rollback` reads it.
- Commits follow Conventional Commits, scope `native`, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-14 by the maintainer's instruction to proceed without waiting: all three as recommended.

1. **Hash tools.** Bash seals through `sha256sum` or `shasum`; the native engine hashes in-process. **Recommended:** accept the deviation ("a missing or failing SHA-256 tool neither fails a native seal nor leaves a snapshot unchecked") and skip the two `rollback_preflight.bats` cases that shim those tools when `AGENTSYNC_NATIVE=1`, with the reason in the skip. Alternative: shell out to the tool from Rust, which adds a process per seal and a platform lookup the migration removes.
2. **The executable bit.** Bash's `[[ -x ]]` asks whether the current user may execute; `std` has no `access(2)`. **Recommended:** `exec` when any execute bit is set, recorded as an accepted deviation (a file executable only by its group or others, owned by the user, records as `exec`). Alternative: learn the effective uid from a temporary file's owner, which is a second side effect per seal.
3. **Where the witness lives.** **Recommended:** `src/witness.rs`, one module per Bash helper as the module map does. Alternative: inside `backup.rs`, which is already 1 200 lines.

## Module closure

```text
lib/helpers/backup_state.sh       1-333     escape, walk, print, seal, first_difference, preflight
lib/helpers/backup.sh             455-460   _backup_snapshot_path refuses symlinked .complete and targets.tsv
                                  722-745   _rollback_cleanup seals the restored safety snapshot
                                  760-805   help text, _rollback_discard_safety, _rollback_report_conflict, --force
                                  861-975   cmd_rollback: preflight, plan, dry-run, final check, seal
lib/sync.sh                       1170-1180 _sync_cleanup seals the restored snapshot
                                  1359-1364 main seals after the transaction
```

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/rollback_preflight.bats | grep '^not ok'
for f in rollback backup backup_retention; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the family 3 close commit; `178 passed` and `11 passed`; the native `rollback_preflight.bats` failures recorded verbatim in the Run log; `0` for `rollback`, `backup`, and `backup_retention`.

---

### Task 1: `src/witness.rs` and the `backup.rs` Helpers It Needs

**Files:**
- Create: `src/witness.rs`
- Modify: `src/lib.rs` (`pub mod witness;`)
- Modify: `src/backup.rs` (`validate_rel`, `safe_target_path`, `create_unique` become `pub(crate)`; `snapshot_path` refuses symlinked `.complete` and `targets.tsv`; new `discard_safety`; test)

**Interfaces:**
- Produces:
  - `pub const SCHEMA: &str = "post-state-v2"`
  - `pub enum Preflight { Clean, Unsealed(&'static str), Conflict(String) }` (`Debug`, `PartialEq`, `Eq`)
  - `pub fn seal(supplied_root: &str, snapshot: &str) -> Result<(), String>` — `Err` is `BACKUP_SEAL_REASON`
  - `pub fn preflight(root: &str, snapshot: &str) -> Preflight` — `root` canonical
  - `backup::discard_safety(store: &str, safety: &str, previous_latest: &str) -> std::io::Result<()>`

- [x] **Step 1: Write the failing tests**

Create `src/witness.rs` with only the tests module and add `pub mod witness;` to `src/lib.rs` after `pub mod version;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::backup::{self, Retention};
    use crate::manifest::sha256_hex;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (dir, root)
    }

    fn write(root: &str, rel: &str, text: &str) {
        let path = format!("{root}/{rel}");
        std::fs::create_dir_all(crate::paths::parent(&path)).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_seal_records_one_line_per_path_like_backup_seal() {
        let (_dir, root) = project();
        write(&root, ".codex/config.toml", "one\n");
        write(&root, ".codex/sub/run.sh", "#!/bin/sh\n");
        std::fs::set_permissions(
            format!("{root}/.codex/sub/run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        symlink("config.toml", format!("{root}/.codex/link")).unwrap();
        write(&root, ".codex/100%", "percent\n");
        let targets = [format!("{root}/.codex"), format!("{root}/absent.md")];
        let snapshot = backup::create(&root, "sync", &targets, Retention::Bounded).unwrap();

        assert_eq!(seal(&root, &snapshot), Ok(()));
        let tsv = std::fs::read(format!("{snapshot}/targets.tsv")).unwrap();
        assert_eq!(
            std::fs::read_to_string(format!("{snapshot}/after.tsv")).unwrap(),
            format!(
                "post-state-v2\t{}\ndir\t-\t.codex\nfile\t{}\t.codex/100%25\nfile\t{}\t.codex/config.toml\nlink\tconfig.toml\t.codex/link\ndir\t-\t.codex/sub\nexec\t{}\t.codex/sub/run.sh\nmissing\t-\tabsent.md\n",
                sha256_hex(&tsv),
                sha256_hex(b"percent\n"),
                sha256_hex(b"one\n"),
                sha256_hex(b"#!/bin/sh\n"),
            )
        );
        assert_eq!(
            seal(&root, &snapshot),
            Err("the snapshot already has a post-operation record".to_string())
        );
        assert_eq!(preflight(&root, &snapshot), Preflight::Clean);
    }

    #[test]
    fn the_preflight_names_the_first_changed_path() {
        let (_dir, root) = project();
        write(&root, ".claude/skills/a/SKILL.md", "a\n");
        write(&root, ".claude/skills/b/SKILL.md", "b\n");
        let targets = [format!("{root}/.claude/skills")];
        let snapshot = backup::create(&root, "sync", &targets, Retention::Bounded).unwrap();
        assert_eq!(
            preflight(&root, &snapshot),
            Preflight::Unsealed("has no post-operation record")
        );
        seal(&root, &snapshot).unwrap();

        write(&root, ".claude/skills/b/SKILL.md", "edited\n");
        assert_eq!(
            preflight(&root, &snapshot),
            Preflight::Conflict(".claude/skills/b/SKILL.md".into())
        );
        write(&root, ".claude/skills/b/SKILL.md", "b\n");
        write(&root, ".claude/skills/a/extra.md", "x\n");
        assert_eq!(
            preflight(&root, &snapshot),
            Preflight::Conflict(".claude/skills/a/extra.md".into())
        );
        std::fs::remove_file(format!("{root}/.claude/skills/a/extra.md")).unwrap();
        std::fs::remove_dir_all(format!("{root}/.claude/skills/b")).unwrap();
        assert_eq!(
            preflight(&root, &snapshot),
            Preflight::Conflict(".claude/skills/b".into())
        );
    }

    #[test]
    fn a_malformed_or_foreign_record_counts_as_unsealed() {
        let (_dir, root) = project();
        write(&root, "CLAUDE.md", "x\n");
        let targets = [format!("{root}/CLAUDE.md")];
        let snapshot = backup::create(&root, "sync", &targets, Retention::Bounded).unwrap();
        let record = format!("{snapshot}/after.tsv");
        for (text, detail) in [
            ("post-state-v2\tnothex\nfile\t-\tCLAUDE.md\n", "has a malformed post-operation record"),
            ("post-state-v2\n", "has a malformed post-operation record"),
            (
                &*format!("post-state-v2\t{}\nfile\t{}\tCLAUDE.md\n", "0".repeat(64), sha256_hex(b"x\n")),
                "has a post-operation record that does not match its target list",
            ),
        ] {
            std::fs::write(&record, text).unwrap();
            assert_eq!(preflight(&root, &snapshot), Preflight::Unsealed(detail));
        }
        let tsv = std::fs::read(format!("{snapshot}/targets.tsv")).unwrap();
        std::fs::write(
            &record,
            format!("post-state-v2\t{}\nfile\tbogus\tCLAUDE.md\n", sha256_hex(&tsv)),
        )
        .unwrap();
        assert_eq!(
            preflight(&root, &snapshot),
            Preflight::Unsealed("has a malformed post-operation record")
        );
    }

    #[test]
    fn a_seal_fails_on_a_trailing_slash_target_or_an_unreadable_file() {
        let (_dir, root) = project();
        write(&root, ".codex/secret", "s\n");
        let targets = [format!("{root}/.codex")];
        let snapshot = backup::create(&root, "sync", &targets, Retention::Bounded).unwrap();
        std::fs::set_permissions(
            format!("{root}/.codex/secret"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        if std::fs::read(format!("{root}/.codex/secret")).is_ok() {
            return;
        }
        assert_eq!(
            seal(&root, &snapshot),
            Err(format!("could not hash {root}/.codex/secret"))
        );
        assert!(!std::path::Path::new(&format!("{snapshot}/after.tsv")).exists());
        let store = crate::paths::parent(&snapshot);
        let leftovers = std::fs::read_dir(&store)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp.seal."))
            .count();
        assert_eq!(leftovers, 0);

        std::fs::write(format!("{snapshot}/targets.tsv"), "present\t.codex/\n").unwrap();
        assert_eq!(
            seal(&root, &snapshot),
            Err("the snapshot target list is invalid".to_string())
        );
    }
}
```

Append inside `mod tests` in `src/backup.rs`:

```rust
    #[cfg(unix)]
    #[test]
    fn a_symlinked_completion_marker_is_not_a_complete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::write(format!("{root}/CLAUDE.md"), "x\n").unwrap();
        let snapshot = create(&root, "sync", &[format!("{root}/CLAUDE.md")], Retention::Bounded).unwrap();
        std::fs::rename(format!("{snapshot}/.complete"), format!("{root}/marker")).unwrap();
        std::os::unix::fs::symlink(format!("{root}/marker"), format!("{snapshot}/.complete")).unwrap();
        let id = paths::leaf(&snapshot);
        assert_eq!(
            snapshot_path(&root, &id).unwrap_err().to_string(),
            format!("Backup snapshot is missing or incomplete: {id}")
        );
    }
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test --lib witness 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: errors for `seal`, `preflight`, and `Preflight` not found.

- [x] **Step 3: Write the implementation**

Above the tests module in `src/witness.rs`:

```rust
//! Post-operation witnesses of `lib/helpers/backup_state.sh`. `<snapshot>/after.tsv`
//! records every target once the operation that took the snapshot finished,
//! bound to its `targets.tsv`; rollback compares the live targets with it.
//! Symlinks are leaves compared by link text.

use std::path::Path;

use crate::manifest::sha256_hex;
use crate::{backup, paths};

pub const SCHEMA: &str = "post-state-v2";

/// `BACKUP_PREFLIGHT_STATUS` with its detail.
#[derive(Debug, PartialEq, Eq)]
pub enum Preflight {
    Clean,
    Unsealed(&'static str),
    Conflict(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Seal,
    Check,
}

struct Record {
    kind: &'static str,
    abs: String,
    path: String,
}

/// `_backup_witness_escape_r`.
fn escape(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// `_backup_witness_walk`: directories in byte order, links never followed.
fn walk(abs: &str, rel: &str, records: &mut Vec<Record>) {
    let record = |kind| Record {
        kind,
        abs: abs.to_string(),
        path: escape(rel),
    };
    let Ok(meta) = std::fs::symlink_metadata(abs) else {
        records.push(record("missing"));
        return;
    };
    let kind = meta.file_type();
    if kind.is_symlink() {
        records.push(record("link"));
    } else if kind.is_file() && is_executable(&meta) {
        records.push(record("exec"));
    } else if kind.is_file() {
        records.push(record("file"));
    } else if kind.is_dir() {
        records.push(record("dir"));
        let mut names: Vec<String> = std::fs::read_dir(abs)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        for name in names {
            walk(&format!("{abs}/{name}"), &format!("{rel}/{name}"), records);
        }
    } else {
        records.push(record("other"));
    }
}

/// `IFS=$'\t' read -r state rel extra`.
fn fields(line: &str) -> (&str, &str, &str) {
    let line = line.trim_matches('\t');
    let (state, rest) = line.split_once('\t').unwrap_or((line, ""));
    let rest = rest.trim_start_matches('\t');
    let (rel, extra) = rest.split_once('\t').unwrap_or((rest, ""));
    (state, rel, extra.trim_start_matches('\t'))
}

/// `_backup_witness_print`: the witness text, or the reason it could not be taken.
fn print(root: &str, targets: &str, mode: Mode) -> Result<String, String> {
    let tsv = Path::new(targets);
    if tsv.is_symlink() || !tsv.is_file() {
        return Err("the snapshot target list is missing".to_string());
    }
    let bytes = std::fs::read(tsv).map_err(|_| format!("could not hash {targets}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    let mut records = Vec::new();
    for line in lines {
        let (state, rel, extra) = fields(line);
        if state.is_empty() && rel.is_empty() && extra.is_empty() {
            continue;
        }
        if (state != "present" && state != "missing")
            || !extra.is_empty()
            || rel.ends_with('/')
            || rel.contains("//")
            || backup::validate_rel(rel, false).is_err()
        {
            return Err("the snapshot target list is invalid".to_string());
        }
        let Ok(abs) = backup::safe_target_path(root, rel, false) else {
            return Err(format!("target is not inside the project: {rel}"));
        };
        walk(&abs, rel, &mut records);
    }

    let mut out = format!("{SCHEMA}\t{}\n", sha256_hex(&bytes));
    for record in records {
        let value = match record.kind {
            "file" | "exec" => match std::fs::read(&record.abs) {
                Ok(content) => sha256_hex(&content),
                Err(_) if mode == Mode::Seal => {
                    return Err(format!("could not hash {}", record.abs));
                }
                Err(_) => "?".to_string(),
            },
            "link" => match std::fs::read_link(&record.abs) {
                Ok(target) => escape(&target.to_string_lossy()),
                Err(_) if mode == Mode::Seal => {
                    return Err(format!("could not read link {}", record.abs));
                }
                Err(_) => "?".to_string(),
            },
            _ => "-".to_string(),
        };
        out.push_str(&format!("{}\t{value}\t{}\n", record.kind, record.path));
    }
    Ok(out)
}

/// `backup_seal`: record the post-operation state once; on failure the
/// snapshot stays unsealed and the reason is returned.
pub fn seal(supplied_root: &str, snapshot: &str) -> Result<(), String> {
    let missing = || "the snapshot is missing or incomplete".to_string();
    let root = backup::canonical_root(supplied_root).map_err(|_| missing())?;
    let snapshot = backup::snapshot_path(&root, snapshot).map_err(|_| missing())?;
    let record = format!("{snapshot}/after.tsv");
    if std::fs::symlink_metadata(&record).is_ok() {
        return Err("the snapshot already has a post-operation record".to_string());
    }
    let stage = backup::create_unique(&paths::parent(&snapshot), ".tmp.seal.", false)
        .map_err(|_| "could not stage the post-operation record".to_string())?;
    let body = match print(&root, &format!("{snapshot}/targets.tsv"), Mode::Seal) {
        Ok(body) => body,
        Err(reason) => {
            let _ = std::fs::remove_file(&stage);
            return Err(reason);
        }
    };
    if std::fs::write(&stage, body)
        .and_then(|()| std::fs::rename(&stage, &record))
        .is_err()
    {
        let _ = std::fs::remove_file(&stage);
        return Err("could not write the post-operation record".to_string());
    }
    Ok(())
}

fn last_field(line: &str) -> &str {
    line.rsplit('\t').next().unwrap_or(line)
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_record(line: &str) -> bool {
    let parts: Vec<&str> = line.split('\t').collect();
    let [kind, value, path] = parts.as_slice() else {
        return false;
    };
    !path.is_empty()
        && match *kind {
            "missing" | "dir" | "other" => *value == "-",
            "file" | "exec" => is_hex64(value),
            "link" => !value.is_empty(),
            _ => false,
        }
}

/// `_backup_witness_first_difference`: `None` when `stored` is malformed.
fn first_difference(stored: &str, current: &str) -> Option<String> {
    let stored: Vec<&str> = stored.split('\n').collect();
    if !stored.iter().all(|line| is_record(line)) {
        return None;
    }
    let current: Vec<&str> = current.split('\n').collect();
    let mut index = 0;
    let (stored_line, current_line) = loop {
        match (stored.get(index), current.get(index)) {
            (None, None) => return None,
            (Some(line), None) | (None, Some(line)) => return Some(last_field(line).to_string()),
            (Some(s), Some(c)) if s != c => break (*s, *c),
            _ => index += 1,
        }
    };
    let stored_path = last_field(stored_line);
    let current_path = last_field(current_line);
    if stored_path != current_path
        && current[index + 1..]
            .iter()
            .any(|line| last_field(line) == stored_path)
    {
        return Some(current_path.to_string());
    }
    Some(stored_path.to_string())
}

fn first_line(text: &str) -> &str {
    text.split_once('\n').map_or(text, |(head, _)| head)
}

fn body(text: &str) -> &str {
    text.split_once('\n').map_or(text, |(_, rest)| rest)
}

/// `backup_preflight`: read-only.
pub fn preflight(root: &str, snapshot: &str) -> Preflight {
    let record = Path::new(snapshot).join("after.tsv");
    if record.is_symlink() || !record.is_file() {
        return Preflight::Unsealed("has no post-operation record");
    }
    let malformed = Preflight::Unsealed("has a malformed post-operation record");
    let bytes = std::fs::read(&record).unwrap_or_default();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]);
    let stored = text.strip_suffix('\n').unwrap_or(&text);
    if !stored.contains('\n') {
        return malformed;
    }
    let header = first_line(stored);
    let well_formed = header
        .strip_prefix(&format!("{SCHEMA}\t"))
        .is_some_and(is_hex64);
    if !well_formed {
        return malformed;
    }
    let Ok(current) = print(root, &format!("{snapshot}/targets.tsv"), Mode::Check) else {
        return Preflight::Unsealed("cannot be checked because its targets could not be inspected");
    };
    let current = current.trim_end_matches('\n');
    if header != first_line(current) {
        return Preflight::Unsealed(
            "has a post-operation record that does not match its target list",
        );
    }
    if stored == current {
        return Preflight::Clean;
    }
    match first_difference(body(stored), body(current)) {
        Some(path) => Preflight::Conflict(path),
        None => malformed,
    }
}
```

In `src/backup.rs`: change `fn validate_rel`, `fn safe_target_path`, and `fn create_unique` to `pub(crate) fn`. In `snapshot_path`, replace the completeness condition with:

```rust
    if path.is_symlink()
        || !path.is_dir()
        || !path.join(".complete").is_file()
        || path.join(".complete").is_symlink()
        || !path.join("targets.tsv").is_file()
        || path.join("targets.tsv").is_symlink()
    {
```

Add after `latest`:

```rust
/// `_rollback_discard_safety`: remove a safety snapshot no restore will use and
/// put `.latest` back.
pub fn discard_safety(store: &str, safety: &str, previous_latest: &str) -> std::io::Result<()> {
    remove_all(Path::new(safety))?;
    if previous_latest.is_empty() {
        return match std::fs::remove_file(format!("{store}/.latest")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    write_store_file(store, ".latest", format!("{previous_latest}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `183 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/witness.rs src/lib.rs src/backup.rs docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md
git commit -m "feat(native): seal and check the post-operation witness"
```

---

### Task 1b: A FIFO Under a Target Is Backed Up Without Being Opened (amended during execution)

Task 0's baseline hung on `sync with a FIFO under a target succeeds and records it by type`: `backup::copy_preserving` called `std::fs::copy` on the FIFO, whose `open` blocks until a writer appears. A trapped `TERM` never took effect, because the process never returned from `open`. Bash copies with `tar` or `cp -pPR`, which recreate the FIFO without reading it.

**Files:**
- Modify: `src/backup.rs` (`copy_preserving`; test)

- [x] **Step 1: Write the failing test**

Append inside `mod tests` in `src/backup.rs`:

```rust
    #[test]
    fn a_fifo_under_a_target_is_recreated_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(format!("{root}/.claude/skills")).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(format!("{root}/.claude/skills/pipe"))
            .status()
            .is_ok_and(|status| status.success());
        if !made {
            return;
        }
        let snapshot = create(
            &root,
            "sync",
            &[format!("{root}/.claude/skills")],
            Retention::Bounded,
        )
        .unwrap();
        use std::os::unix::fs::FileTypeExt;
        let copied = std::fs::symlink_metadata(format!("{snapshot}/files/.claude/skills/pipe")).unwrap();
        assert!(copied.file_type().is_fifo());
        std::fs::remove_file(format!("{root}/.claude/skills/pipe")).unwrap();
        restore(&root, &snapshot).unwrap();
        let restored = std::fs::symlink_metadata(format!("{root}/.claude/skills/pipe")).unwrap();
        assert!(restored.file_type().is_fifo());
    }
```

- [x] **Step 2: Run it, confirm it fails**

Run: `cargo test --lib backup::tests::a_fifo 2>&1 | tail -3` in the background and stop it after 20 seconds.
Expected: it does not finish (the copy blocks in `open`).

- [x] **Step 3: Write the implementation**

In `copy_preserving`, before `std::fs::copy(src, dst)?;`:

```rust
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let kind = meta.file_type();
        if kind.is_fifo() {
            let made = std::process::Command::new("mkfifo").arg(dst).status()?;
            if !made.success() {
                return Err(std::io::Error::other(format!(
                    "mkfifo failed for {}",
                    dst.display()
                )));
            }
            return std::fs::set_permissions(dst, meta.permissions());
        }
        if kind.is_socket() || kind.is_block_device() || kind.is_char_device() {
            return Ok(());
        }
    }
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `184 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/backup.rs docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md
git commit -m "fix(native): recreate a FIFO in a backup instead of reading it"
```

Every later count in this plan is one higher.

---

### Task 2: `sync` Seals Its Snapshot

**Files:**
- Modify: `src/cli/sync.rs` (`Transaction::fail`, `sync`)

**Interfaces:**
- Consumes: `witness::seal`.

- [x] **Step 1: Write the implementation**

In `Transaction::fail`, replace the `Ok(())` arm with:

```rust
            Ok(()) => {
                if let Err(reason) = witness::seal(&root, &backup_path) {
                    s.log.warning(&format!(
                        "Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.",
                        paths::leaf(&backup_path)
                    ));
                }
                s.log.info(&format!("Restored pre-sync state from {shown}"));
                prune(s, env, self.retention);
            }
```

At the end of `sync`, replace `tx.active = false;\n    Ok(())` with:

```rust
    tx.active = false;
    if let Some(backup_path) = &tx.backup
        && let Err(reason) = witness::seal(&root, backup_path)
    {
        s.log.warning(&format!(
            "Could not record the post-sync state ({reason}); rolling back backup {} cannot detect later changes.",
            paths::leaf(backup_path)
        ));
    }
    Ok(())
```

Add `witness` to the `use crate::{…}` list of `src/cli/sync.rs`.

- [x] **Step 2: Run the tests**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
for f in sync backup_retention drift; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'native sync writes is restored' tests/native_parity.bats
```

Expected: `184 passed` and `11 passed`; `0` for each file; the parity case `ok`.

- [x] **Step 3: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/sync.rs docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md
git commit -m "feat(native): seal the sync snapshot after the run"
```

---

### Task 3: `rollback` Checks the Witness and Takes `--force`

**Files:**
- Modify: `src/cli/rollback.rs` (`USAGE`, `run`, new `report_conflict`; tests)

**Interfaces:**
- Consumes: `witness::{seal, preflight, Preflight}`, `backup::discard_safety`.

- [x] **Step 1: Write the failing test**

In the tests module of `src/cli/rollback.rs`, in `a_rollback_restores_the_latest_backup_and_leaves_an_undo_backup`, seal the snapshot once the test has written `after\n` and created `.claude/rules`, which stand for the operation the snapshot guarded:

```rust
        crate::witness::seal(&root, &snapshot).unwrap();
```

and append a new test:

```rust
    #[test]
    fn a_changed_target_refuses_and_force_restores_it() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md")];
        let snapshot = backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "synced\n").unwrap();
        crate::witness::seal(&root, &snapshot).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "edited\n").unwrap();

        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.out, "");
        assert_eq!(
            refused.err,
            format!(
                "Error: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; no files were changed.\nRe-run with --force to restore the backup anyway and discard that change.\n"
            )
        );
        let dry = rollback(&root, &["--dry-run", "--force"], true);
        assert_eq!(dry.status, 0);
        assert_eq!(
            dry.err,
            format!(
                "Warning: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; --force will overwrite it.\n"
            )
        );

        let forced = rollback(&root, &["--force", "--yes"], true);
        assert_eq!(forced.status, 0);
        assert_eq!(forced.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(std::path::Path::new(&format!("{undo}/after.tsv")).is_file());

        std::fs::remove_file(format!("{undo}/after.tsv")).unwrap();
        let unsealed = rollback(&root, &["--yes"], true);
        assert_eq!(unsealed.status, 0);
        assert!(unsealed.err.starts_with(&format!(
            "Warning: Backup {} has no post-operation record; changes made after that operation cannot be detected.\n",
            paths::leaf(&undo)
        )));
    }
```

- [x] **Step 2: Run it, confirm it fails**

Run: `cargo test --lib cli::rollback 2>&1 | grep -E '^test |panicked'`
Expected: `a_changed_target_refuses_and_force_restores_it ... FAILED` (`--force` is an unknown option).

- [x] **Step 3: Write the implementation**

Replace `USAGE`'s text after the first paragraph with:

```text
Rollback refuses, naming the first changed path, when a target differs from the
state recorded after the backup's operation finished.

Options:
  --list       List complete backups
  --dry-run    Show the restore plan and any conflict without changing files
  --force      Restore even when targets changed after the backup's operation
  -y, --yes    Skip the confirmation prompt
  -h, --help   Show this help
```

In `run`: add `mut force` to the flags, `"--force" => force = true,` to the argument match, and `use crate::witness::{self, Preflight};`. After `let targets = …;`:

```rust
    let is_latest = matches!(backup::latest(&root), Ok(Some(latest)) if paths::leaf(&latest) == id);
    let (mut sealed, mut conflict) = (false, None);
    if !force || dry_run {
        match witness::preflight(&root, &snapshot) {
            Preflight::Clean => sealed = true,
            Preflight::Conflict(path) => conflict = Some(path),
            Preflight::Unsealed(detail) => {
                let _ = writeln!(
                    err,
                    "Warning: Backup {id} {detail}; changes made after that operation cannot be detected."
                );
            }
        }
    }
    if let Some(path) = &conflict
        && !dry_run
    {
        report_conflict(err, &id, path, is_latest);
        return 1;
    }
```

Replace the dry-run block with:

```rust
    if dry_run {
        match &conflict {
            Some(path) if force => {
                let _ = writeln!(
                    err,
                    "Warning: Rollback conflict: {path} changed after the operation recorded in backup {id}; --force will overwrite it."
                );
            }
            Some(path) => report_conflict(err, &id, path, is_latest),
            None => {}
        }
        let _ = writeln!(out, "Dry run — nothing was written.");
        return if conflict.is_some() && !force { 1 } else { 0 };
    }
```

Before `let safety = …`, read the previous pointer:

```rust
    let store = format!("{root}/.ai/backups");
    let pointer = std::path::Path::new(&store).join(".latest");
    let previous_latest = if pointer.is_file() && !pointer.is_symlink() {
        std::fs::read(&pointer)
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
```

After the safety snapshot is created and before `Interrupt::arm()`:

```rust
    if sealed && let Preflight::Conflict(path) = witness::preflight(&root, &snapshot) {
        if backup::discard_safety(&store, &safety, &previous_latest).is_err() {
            let _ = writeln!(
                err,
                "Warning: Could not remove the unused safety backup {}.",
                paths::leaf(&safety)
            );
        }
        report_conflict(err, &id, &path, is_latest);
        return 1;
    }
```

In the recovery branch, inside `Ok(())` of `backup::restore(&root, &safety)`, before the `Restored pre-rollback state` line:

```rust
                if let Err(reason) = witness::seal(&root, &safety) {
                    let _ = writeln!(
                        err,
                        "Warning: Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.",
                        paths::leaf(&safety)
                    );
                }
```

After `drop(interrupt);`, before the prune:

```rust
    if let Err(reason) = witness::seal(&root, &safety) {
        let _ = writeln!(
            err,
            "Warning: Could not record the post-rollback state ({reason}); rolling back backup {} cannot detect later changes.",
            paths::leaf(&safety)
        );
    }
```

Add at module level:

```rust
/// `_rollback_report_conflict`.
fn report_conflict(err: &mut dyn Write, id: &str, path: &str, is_latest: bool) {
    let _ = writeln!(
        err,
        "Error: Rollback conflict: {path} changed after the operation recorded in backup {id}; no files were changed."
    );
    let _ = if is_latest {
        writeln!(err, "Re-run with --force to restore the backup anyway and discard that change.")
    } else {
        writeln!(
            err,
            "Newer AgentSync operations may have changed this target. Roll back the newer backups first, or re-run with --force to restore anyway."
        )
    };
}
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/rollback_preflight.bats | grep '^not ok'
for f in rollback backup backup_retention baseline; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `185 passed` and `11 passed`; in `rollback_preflight.bats` only the two hash-tool cases fail (Task 4 gates them); `0` for the others.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/rollback.rs docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md
git commit -m "feat(native): refuse a rollback over changed targets and add --force"
```

---

### Task 4: Deviations, Parity Fixtures, and Module Map

**Files:**
- Modify: `tests/rollback_preflight.bats` (two native skips), `tests/native_parity.bats`, `docs/specs/2026-09-12-rust-migration-design.md` (Accepted deviations), `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Gate the hash-tool cases**

At the top of `@test "a seal that cannot hash warns and keeps the completed sync"` and `@test "the seal stages its record where stale-staging sweeps reclaim it"` in `tests/rollback_preflight.bats`:

```bash
    [[ "$AGENTSYNC_NATIVE" != 1 ]] || skip "the native engine hashes in-process"
```

Append to "Accepted deviations" in the design spec:

```markdown
- Phase 3b: the native engine hashes `after.tsv` in-process, so a missing or
  failing `sha256sum` or `shasum` neither fails a seal nor leaves a rollback
  unchecked.
- Phase 3b: `after.tsv` records a regular file as `exec` when any execute bit
  is set; Bash's `[[ -x ]]` asked whether the current user may execute it.
```

- [ ] **Step 2: Write the fixtures**

After `parity: rollback plans, restores, and refuses like Bash`:

```bash
@test "parity: rollback conflicts, --force, and unsealed snapshots" {
    enable_tools claude
    _bash_sync
    printf 'edited\n' >> CLAUDE.md
    assert_tree_parity rollback --yes
    assert_tree_parity rollback --dry-run
    assert_tree_parity rollback --dry-run --force
    assert_tree_parity rollback --force --yes
    rm ".ai/backups/$(cat .ai/backups/.latest)/after.tsv"
    assert_tree_parity rollback --yes
    printf 'post-state-v2\tbad\n' > ".ai/backups/$(cat .ai/backups/.latest)/after.tsv"
    assert_tree_parity rollback --yes
}
```

In `parity: a backup the native sync writes is restored by the Bash rollback`, after the `targets.tsv` comparison:

```bash
    cmp "$right/.ai/backups/$(cat "$right/.ai/backups/.latest")/after.tsv" \
        "$left/.ai/backups/$(cat "$left/.ai/backups/.latest")/after.tsv"
```

- [ ] **Step 3: Run them and prove they bite**

```bash
cargo build --release
bats --tap -f 'rollback' tests/native_parity.bats
```

Expected: every case `ok`. Then change `"; no files were changed."` to `"; nothing was changed."` in `src/cli/rollback.rs`, rebuild, rerun `-f 'rollback conflicts'`: `not ok` with that diff; revert and rebuild.

- [ ] **Step 4: Module map and outputs**

In `.ai/src/skills/native-port/references/module-map.md`, add after the `lib/helpers/backup.sh` row:

```text
lib/helpers/backup_state.sh      → src/witness.rs          after.tsv post-state-v2: print, seal, preflight, first difference
```

and extend the `lib/helpers/backup.sh (rollback)` row with `; preflight, --force, sealed safety snapshot`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 5: Verify the family and Phase 3b**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in rollback_preflight rollback backup backup_retention baseline config_safety version_pin source_overrides sync check list native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `185 passed` and `11 passed`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 6: Commit**

```bash
git add tests/rollback_preflight.bats tests/native_parity.bats docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md
git commit -m "test(native): diff Bash against the native rollback witness"
```

---

## Completion

The family is closed when every box is ticked, `rollback_preflight.bats` is green under `AGENTSYNC_NATIVE=1` (two cases skipped by decision 1), `tests/native_parity.bats` is green in both modes, and a `## Completion receipt` records the fresh verification. With it Phase 3b is closed; the next unit of work is Phase 4's first plan.

## Run log

### 2026-09-14 — Phase 3b family 4 planned
- Commits: this plan.
- Verified: plan written against `lib/helpers/backup_state.sh`, `git diff 0.35.2 0.36.0 -- lib/helpers/backup.sh lib/sync.sh`, `tests/rollback_preflight.bats`, and `src/backup.rs`, `src/cli/rollback.rs`, `src/cli/sync.rs`, `src/manifest.rs`.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-14 — Tasks 0 and 1 done
- Commits: "feat(native): seal and check the post-operation witness".
- Verified: Task 0 at `b9669f6`: `cargo test` 178 and 11; native `rollback_preflight` 23 failures (1–8, 11–16, 18–20, 22, 26–30), `rollback`, `backup`, `backup_retention` 0. Case 28 hung the native `sync` for 21 minutes and a `sync` from the earlier post-merge baseline had hung for almost 6 hours: that run, recorded on 2026-09-14 as killed for low memory, was this hang; both were ended with `kill -9`, since the blocked `open` never reaches the signal flag. Task 1: `cargo test` 183 and 11 passed; fmt and clippy exit 0.
- Plan amended: Task 1b added for the FIFO; later counts are one higher.
- Next: Task 1b Step 1.
- Blocker: none.

### 2026-09-14 — Tasks 1b and 2 done
- Commits: "fix(native): recreate a FIFO in a backup instead of reading it", "feat(native): seal the sync snapshot after the run".
- Verified: Task 1b's test ran past 40 seconds before the fix and was killed, then passed in 0.01 s; `cargo test` 184 and 11. Task 2: native `sync`, `backup_retention`, `drift` 0; `parity: a backup the native sync writes is restored by the Bash rollback` `ok` with Task 4's `cmp` of the two `after.tsv` files already in place (left uncommitted until Task 4).
- Plan amended: none.
- Next: Task 3 Step 4 (Steps 1–3 are written and `cargo test` passes 185 and 11).
- Blocker: none.

### 2026-09-14 — Task 3 done
- Commits: "feat(native): refuse a rollback over changed targets and add --force".
- Verified: `cargo test` 185 and 11; fmt and clippy exit 0. Native bats: `rollback_preflight` 2 failures (29 and 30, the hash-tool shims Task 4 gates), `rollback`, `backup`, `backup_retention`, `baseline` 0.
- Plan amended: none.
- Next: Task 4 Step 1 (the spec deviations, the parity fixture, and the module map rows are written; Task 1b adds a third deviation line for sockets and devices).
- Blocker: none.
