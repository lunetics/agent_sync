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

    /// The manifest's `hashed_lines`.
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            entries: hashed_lines(bytes),
        }
    }

    pub fn paths(&self) -> BTreeSet<String> {
        self.entries.iter().map(|(rel, _)| rel.clone()).collect()
    }

    /// `MANIFEST_KEYS` and `MANIFEST_VALUES`, in file order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
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

/// `manifest_update_entry`: the manifest rewritten with `<rel>\t<hash>` in place
/// of any line for `rel`, every other line kept as `read` split it, `sort -u`.
pub fn update_entry(root: &str, rel: &str, hash: &str) -> Result<(), Error> {
    if rel.is_empty() || hash.is_empty() {
        return Ok(());
    }
    let path = Path::new(root).join(REL);
    let ai = Path::new(root).join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    let mut lines = BTreeSet::new();
    if path.is_file() {
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        for line in String::from_utf8_lossy(&bytes).split('\n') {
            let line = line.trim_matches('\t');
            let (existing, existing_hash) = match line.find('\t') {
                Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
                None => (line, ""),
            };
            if existing.is_empty() || existing == rel {
                continue;
            }
            lines.insert(format!("{existing}\t{existing_hash}"));
        }
    }
    lines.insert(format!("{rel}\t{hash}"));
    let mut text = lines.into_iter().collect::<Vec<_>>().join("\n");
    text.push('\n');
    staging::write_beside(&path, text.as_bytes())
}

/// `IFS=$'\t' read -r rel hash` per line: tabs around the line are dropped, the
/// hash is the rest after the first run of tabs, and comments and entries
/// without a hash are skipped.
pub(crate) fn hashed_lines(bytes: &[u8]) -> Vec<(String, String)> {
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
    entries
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
    use crate::paths::DiskText;

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
    fn one_entry_is_replaced_and_every_other_line_is_kept_as_bash_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(REL),
            "z.md\tzz\n# note\t\nb.md\told\n\ta.md\t\taa\t\nnohash\nb.md\tdup\n",
        )
        .unwrap();
        update_entry(&root, "b.md", "new").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "# note\t\na.md\taa\nb.md\tnew\nnohash\t\nz.md\tzz\n"
        );
        update_entry(&root, "", "x").unwrap();
        update_entry(&root, "c.md", "").unwrap();
        assert!(
            std::fs::read_to_string(dir.path().join(REL))
                .unwrap()
                .ends_with("z.md\tzz\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn drift_is_a_changed_file_in_manifest_order_and_a_missing_file_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
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
        let root = dir.path().disk_text();
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
