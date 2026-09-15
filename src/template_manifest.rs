//! `lib/helpers/template_manifest.sh`: content hashes of the templates copied
//! into `.ai/src/`.

use std::path::Path;

use crate::manifest::sha256_hex;

/// `template_manifest_hash`: the SHA-256 of a file, links followed; `None`
/// when the path is not a readable regular file.
pub fn hash(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
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
}
