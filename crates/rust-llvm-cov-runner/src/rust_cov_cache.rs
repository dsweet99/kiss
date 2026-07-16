use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use crate::{CACHE_SCHEMA_VERSION, RustLineCoverage, RustLlvmCovOutcome};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RustCovCacheEntry {
    pub(crate) schema_version: String,
    #[serde(default)]
    pub generation_fingerprint: String,
    pub(crate) selector: String,
    pub(crate) status: TestStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: std::time::Duration,
    pub(crate) coverage: RustLineCoverage,
    pub(crate) test_binary_ids: Vec<String>,
}

impl RustCovCacheEntry {
    pub fn from_outcome(outcome: &RustLlvmCovOutcome, generation_fingerprint: &str) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION.to_string(),
            generation_fingerprint: generation_fingerprint.to_string(),
            selector: outcome.selector.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: outcome.coverage.clone(),
            test_binary_ids: outcome.test_binary_ids.clone(),
        }
    }
}

impl From<&RustLlvmCovOutcome> for RustCovCacheEntry {
    fn from(outcome: &RustLlvmCovOutcome) -> Self {
        Self::from_outcome(outcome, "")
    }
}

pub(crate) fn load_rust_cov_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
) -> Option<RustCovCacheEntry> {
    let path = rust_cov_cache_entry_path(cache_root, fingerprint);
    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntry = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == CACHE_SCHEMA_VERSION).then_some(entry)
}

pub fn repo_relative_coverage_file(source_root: &Path, file: &str) -> Option<String> {
    let rel = repo_relative_path(source_root, Path::new(file))?;
    (rel.ends_with(".rs") && !rel.starts_with(".kiss/") && !rel.starts_with('<')).then_some(rel)
}

pub fn repo_relative_path(source_root: &Path, path: &Path) -> Option<String> {
    let root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    let candidate = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let joined = root.join(path);
        joined.canonicalize().unwrap_or(joined)
    };
    let rel = candidate.strip_prefix(&root).ok()?;
    if rel.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

pub fn generation_entries_fingerprint(cache_root: &Path, generation: &str) -> io::Result<String> {
    let entries_dir = cache_root.join("entries");
    let mut names = Vec::new();
    if entries_dir.is_dir() {
        for entry in fs::read_dir(&entries_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(parsed) = serde_json::from_slice::<RustCovCacheEntry>(&fs::read(&path)?) else {
                continue;
            };
            if parsed.generation_fingerprint == generation
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    let mut h = rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    for name in &names {
        h = rust_cov_fnv1a64(h, name.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub fn store_rust_cov_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
    entry: &RustCovCacheEntry,
) -> io::Result<()> {
    let path = rust_cov_cache_entry_path(cache_root, fingerprint);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp_path = parent.join(format!(".{}.{}.tmp", fingerprint, rust_cov_unique_suffix()));
    let mut file = create_new_cache_file(&tmp_path)?;
    serde_json::to_writer(&mut file, entry).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, path)
}

pub(crate) fn create_new_cache_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn rust_cov_cache_entry_path(cache_root: &Path, fingerprint: &str) -> PathBuf {
    cache_root
        .join("entries")
        .join(format!("{fingerprint}.json"))
}

pub(crate) fn rust_cov_unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

pub(crate) fn rust_cov_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    fn outcome() -> RustLlvmCovOutcome {
        RustLlvmCovOutcome {
            selector: "smoke_sub".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(3),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1, 2]))]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: crate::RustCovCacheStatus::MissStored,
            stdout: Some(b"out".to_vec()),
            stderr: Some(b"err".to_vec()),
        }
    }

    #[test]
    fn store_and_load_rust_cov_cache_entry_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = RustCovCacheEntry::from(&outcome());

        store_rust_cov_cache_entry(tmp.path(), "abc123", &entry).unwrap();
        let loaded = load_rust_cov_cache_entry(tmp.path(), "abc123").unwrap();

        assert_eq!(loaded.selector, "smoke_sub");
        assert_eq!(loaded.status, TestStatus::Passed);
        assert_eq!(loaded.coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
        assert!(rust_cov_cache_entry_path(tmp.path(), "abc123").ends_with("entries/abc123.json"));
    }

    #[test]
    fn load_rust_cov_cache_entry_ignores_bad_json_and_wrong_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let bad_path = rust_cov_cache_entry_path(tmp.path(), "bad");
        fs::create_dir_all(bad_path.parent().unwrap()).unwrap();
        fs::write(&bad_path, "{not json").unwrap();
        assert!(load_rust_cov_cache_entry(tmp.path(), "bad").is_none());

        let wrong_schema = serde_json::json!({
            "schema_version": "old",
            "selector": "smoke_sub",
            "status": "Passed",
            "exit_code": 0,
            "duration": { "secs": 0, "nanos": 1 },
            "test_binary_ids": ["test-bin"],
            "coverage": { "files": {} }
        });
        fs::write(
            rust_cov_cache_entry_path(tmp.path(), "old"),
            wrong_schema.to_string(),
        )
        .unwrap();
        assert!(load_rust_cov_cache_entry(tmp.path(), "old").is_none());
    }

    #[test]
    fn conservative_inputs_include_rust_metadata_and_skip_cache_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
        fs::create_dir_all(tmp.path().join("target")).unwrap();
        fs::create_dir_all(tmp.path().join(".kiss").join("rust_llvm_cov_cache")).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(tmp.path().join("Cargo.lock"), "# lock\n").unwrap();
        fs::write(tmp.path().join("rust-toolchain.toml"), "[toolchain]\n").unwrap();
        fs::write(tmp.path().join(".cargo").join("config.toml"), "[build]\n").unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn value() {}\n").unwrap();
        fs::write(tmp.path().join("target").join("ignored.rs"), "ignored\n").unwrap();
        fs::write(
            tmp.path()
                .join(".kiss")
                .join("rust_llvm_cov_cache")
                .join("ignored.rs"),
            "ignored\n",
        )
        .unwrap();

        let names: BTreeSet<_> = crate::shared_input::rust_cov_input_files(tmp.path())
            .unwrap()
            .into_iter()
            .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
            .collect();

        assert!(names.contains(Path::new("Cargo.toml")));
        assert!(names.contains(Path::new("Cargo.lock")));
        assert!(names.contains(Path::new("rust-toolchain.toml")));
        assert!(names.contains(Path::new(".cargo/config.toml")));
        assert!(names.contains(Path::new("src/lib.rs")));
        assert!(!names.contains(Path::new("target/ignored.rs")));
        assert!(!names.contains(Path::new(".kiss/rust_llvm_cov_cache/ignored.rs")));
        assert!(crate::shared_input::should_skip_rust_cov_dir(
            &tmp.path().join("target")
        ));
        assert!(crate::shared_input::is_kiss_rust_cov_cache_dir(
            &tmp.path().join(".kiss").join("rust_llvm_cov_cache")
        ));
    }

    #[test]
    fn helper_hash_and_temp_suffix_are_usable() {
        assert_ne!(rust_cov_unique_suffix(), "");
        assert_eq!(
            rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"hello"),
            0xa430_d846_80aa_bd0b
        );
        assert_ne!(
            rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
            rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
        );
    }
}
