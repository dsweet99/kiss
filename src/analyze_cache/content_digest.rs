use kiss::check_universe_cache::FullCheckCache;
use std::path::PathBuf;

use super::fnv1a64;
use super::path_helpers::load_full_cache;

fn content_digest(bytes: &[u8]) -> u64 {
    fnv1a64(0, bytes)
}

pub(super) fn content_digests_for_paths(paths: &[PathBuf]) -> Vec<(String, u64)> {
    let mut digests: Vec<(String, u64)> = paths
        .iter()
        .filter_map(|p| {
            std::fs::read(p).ok().map(|bytes| {
                (
                    p.to_string_lossy().to_string(),
                    content_digest(&bytes),
                )
            })
        })
        .collect();
    digests.sort_by(|a, b| a.0.cmp(&b.0));
    digests
}

pub(crate) fn verify_content_digests(
    stored: &[(String, u64)],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> bool {
    if stored.is_empty() {
        return true;
    }
    let all_paths: Vec<&PathBuf> = py_files.iter().chain(rs_files).collect();
    if all_paths.len() != stored.len() {
        return false;
    }
    let stored_map: std::collections::HashMap<&str, u64> = stored
        .iter()
        .map(|(path, digest)| (path.as_str(), *digest))
        .collect();
    for p in all_paths {
        let key = p.to_string_lossy();
        let Some(stored_digest) = stored_map.get(key.as_ref()) else {
            return false;
        };
        if let Ok(bytes) = std::fs::read(p)
            && content_digest(&bytes) != *stored_digest
        {
            return false;
        }
    }
    true
}

pub(crate) fn load_verified_full_cache(
    fingerprint: &str,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Option<FullCheckCache> {
    let cache = load_full_cache(fingerprint)?;
    if !verify_content_digests(&cache.file_content_digests, py_files, rs_files) {
        return None;
    }
    Some(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn verify_content_digests_rejects_same_metadata_different_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("f.py");

        let content1 = "# pad\n\ndef foo():\n    return 1\n";
        let content2 = "# pad\n\ndef bar():\n    return 2\n";
        assert_eq!(content1.len(), content2.len());

        fs::write(&path, content1).unwrap();
        let stored = content_digests_for_paths(std::slice::from_ref(&path));
        assert!(verify_content_digests(&stored, std::slice::from_ref(&path), &[]));

        fs::write(&path, content2).unwrap();
        assert!(
            !verify_content_digests(&stored, std::slice::from_ref(&path), &[]),
            "content change must fail digest verification even when size and mtime are unchanged"
        );
    }
}
