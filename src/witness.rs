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
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
            (Some(line), None) | (None, Some(line)) => {
                return Some(last_field(line).to_string());
            }
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
        let foreign = format!(
            "post-state-v2\t{}\nfile\t{}\tCLAUDE.md\n",
            "0".repeat(64),
            sha256_hex(b"x\n")
        );
        for (text, detail) in [
            (
                "post-state-v2\tnothex\nfile\t-\tCLAUDE.md\n",
                "has a malformed post-operation record",
            ),
            ("post-state-v2\n", "has a malformed post-operation record"),
            (
                foreign.as_str(),
                "has a post-operation record that does not match its target list",
            ),
        ] {
            std::fs::write(&record, text).unwrap();
            assert_eq!(preflight(&root, &snapshot), Preflight::Unsealed(detail));
        }
        let tsv = std::fs::read(format!("{snapshot}/targets.tsv")).unwrap();
        std::fs::write(
            &record,
            format!(
                "post-state-v2\t{}\nfile\tbogus\tCLAUDE.md\n",
                sha256_hex(&tsv)
            ),
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
