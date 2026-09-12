use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const PRESERVED_CACHE_DIRS: &[&str] = &["build", "locks"];
pub const PRESERVED_CACHE_FILES: &[&str] = &["binary_digest_memo.json", "runner_resolve_cache.json"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SemanticClearReport {
    pub removed_entries: usize,
    pub preserved_build: bool,
    pub preserved_locks: bool,
}

pub fn clear_semantic_evidence(cache_root: &Path) -> io::Result<SemanticClearReport> {
    if !cache_root.is_dir() {
        return Ok(SemanticClearReport::default());
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(cache_root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if PRESERVED_CACHE_DIRS.contains(&name) || PRESERVED_CACHE_FILES.contains(&name) {
            continue;
        }
        remove_entry(&entry.path())?;
        removed += 1;
    }
    Ok(SemanticClearReport {
        removed_entries: removed,
        preserved_build: cache_root.join("build").exists(),
        preserved_locks: cache_root.join("locks").exists(),
    })
}

fn remove_entry(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn build_depot_target(cache_root: &Path) -> PathBuf {
    cache_root.join("build").join("target")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn clear_semantic_evidence_preserves_build_and_locks() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("rust_llvm_cov_cache");
        fs::create_dir_all(cache.join("build").join("target")).unwrap();
        fs::create_dir_all(cache.join("locks")).unwrap();
        fs::create_dir_all(cache.join("entries")).unwrap();
        fs::create_dir_all(cache.join("generations")).unwrap();
        fs::write(cache.join("index.json"), b"{}").unwrap();
        fs::write(cache.join("build").join("identity.json"), b"{}").unwrap();
        fs::write(cache.join("binary_digest_memo.json"), b"[]").unwrap();
        fs::write(cache.join("runner_resolve_cache.json"), b"{}").unwrap();

        let report = clear_semantic_evidence(&cache).unwrap();

        assert!(report.preserved_build);
        assert!(report.preserved_locks);
        assert!(report.removed_entries >= 3);
        assert!(cache.join("build").join("target").is_dir());
        assert!(cache.join("build").join("identity.json").is_file());
        assert!(cache.join("locks").is_dir());
        assert!(cache.join("binary_digest_memo.json").is_file());
        assert!(cache.join("runner_resolve_cache.json").is_file());
        assert!(!cache.join("entries").exists());
        assert!(!cache.join("generations").exists());
        assert!(!cache.join("index.json").exists());
    }

    #[test]
    fn clear_semantic_evidence_on_missing_root_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let report = clear_semantic_evidence(&tmp.path().join("missing")).unwrap();
        assert_eq!(report, SemanticClearReport::default());
    }
}
