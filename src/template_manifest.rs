//! `lib/helpers/template_manifest.sh`: content hashes of the templates copied
//! into `.ai/src/`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::manifest::{hashed_lines, sha256_hex};
use crate::{Error, staging};

pub const REL: &str = ".ai/.template-manifest";

/// `template_manifest_hash`: the SHA-256 of a file, links followed; `None`
/// when the path is not a readable regular file.
pub fn hash(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// `TEMPLATE_MANIFEST_KEYS` and `TEMPLATE_MANIFEST_VALUES`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TemplateManifest {
    entries: Vec<(String, String)>,
}

impl TemplateManifest {
    /// `template_manifest_load`: empty when the file is missing.
    pub fn load(root: &Path) -> Result<Self, Error> {
        let path = root.join(REL);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        Ok(Self {
            entries: hashed_lines(&bytes),
        })
    }

    /// `template_manifest_lookup`: the first hash recorded for `rel`.
    pub fn lookup(&self, rel: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == rel)
            .map(|(_, hash)| hash.as_str())
    }

    /// `template_manifest_remove`: every entry for `rel`.
    pub fn remove(&mut self, rel: &str) {
        self.entries.retain(|(key, _)| key != rel);
    }

    /// `template_manifest_write`: `sort -u` lines, or no file when empty.
    pub fn write(&self, root: &Path) -> Result<(), Error> {
        let ai = root.join(".ai");
        std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
        let path = root.join(REL);
        if self.entries.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(&path, e)),
                _ => Ok(()),
            };
        }
        let lines: BTreeSet<String> = self
            .entries
            .iter()
            .map(|(rel, hash)| format!("{rel}\t{hash}"))
            .collect();
        let mut text = lines.into_iter().collect::<Vec<_>>().join("\n");
        text.push('\n');
        staging::write_beside(&path, text.as_bytes())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_file_hashes_as_sha256sum_prints_it_and_anything_else_has_no_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        std::fs::write(&file, "shared rule\n").unwrap();
        assert_eq!(
            hash(&file).as_deref(),
            Some("a5aa98439217de45641258de8f69aea33202a07829b4acf375b5484860ca05b8")
        );
        assert_eq!(hash(dir.path()), None);
        assert_eq!(hash(&dir.path().join("missing.md")), None);
    }

    #[test]
    fn entries_load_look_up_drop_and_write_back_sorted_like_bash() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            TemplateManifest::load(dir.path()).unwrap(),
            TemplateManifest::default()
        );
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(REL),
            "z.md\tzz\n# c\th\nskills/a/SKILL.md\t1\nnohash\n\tb.md\t\tbb\t\nskills/a/SKILL.md\t2\n",
        )
        .unwrap();
        let mut manifest = TemplateManifest::load(dir.path()).unwrap();
        assert_eq!(manifest.lookup("skills/a/SKILL.md"), Some("1"));
        assert_eq!(manifest.lookup("nohash"), None);
        manifest.remove("skills/a/SKILL.md");
        manifest.write(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "b.md\tbb\nz.md\tzz\n"
        );
        manifest.remove("b.md");
        manifest.remove("z.md");
        manifest.write(dir.path()).unwrap();
        assert!(!dir.path().join(REL).exists());
    }
}
