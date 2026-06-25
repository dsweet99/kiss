use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};

use crate::{CACHE_SCHEMA_VERSION, RustLineCoverage, RustLlvmCovOutcome, RustLlvmCovRequest};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RustCovCacheEntry {
    schema_version: String,
    pub(crate) selector: String,
    pub(crate) status: TestStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: std::time::Duration,
    pub(crate) coverage: RustLineCoverage,
}

impl From<&RustLlvmCovOutcome> for RustCovCacheEntry {
    fn from(outcome: &RustLlvmCovOutcome) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION.to_string(),
            selector: outcome.selector.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: outcome.coverage.clone(),
        }
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

pub(crate) fn store_rust_cov_cache_entry(
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

pub(crate) fn rust_cov_fingerprint(req: &RustLlvmCovRequest) -> io::Result<String> {
    let mut h = rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    h = rust_cov_fnv1a64(h, req.selector.as_bytes());
    h = rust_cov_fnv1a64(h, req.cargo.to_string_lossy().as_bytes());
    h = rust_cov_fnv1a64(h, req.llvm_cov_version.as_bytes());
    h = rust_cov_fnv1a64(h, req.rustc_version.as_bytes());
    h = rust_cov_fnv1a64(h, req.cwd.to_string_lossy().as_bytes());
    h = rust_cov_fnv1a64(h, req.source_root.to_string_lossy().as_bytes());
    for arg in &req.cargo_args {
        h = rust_cov_fnv1a64(h, arg.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
    }
    for arg in &req.test_args {
        h = rust_cov_fnv1a64(h, arg.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
    }
    for (key, value) in &req.env {
        h = rust_cov_fnv1a64(h, key.as_bytes());
        h = rust_cov_fnv1a64(h, b"=");
        h = rust_cov_fnv1a64(h, value.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
    }
    for file in rust_cov_input_files(&req.cwd)? {
        h = rust_cov_fnv1a64(h, file.to_string_lossy().as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
        h = rust_cov_fnv1a64(h, &fs::read(file)?);
        h = rust_cov_fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn rust_cov_input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_rust_cov_inputs(root, &mut out)?;
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    Ok(out)
}

fn visit_rust_cov_inputs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_rust_cov_dir(&path) {
                continue;
            }
            visit_rust_cov_inputs(&path, out)?;
        } else if file_type.is_file() && is_rust_cov_cache_input(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn should_skip_rust_cov_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target" | ".rust_llvm_cov_cache")
    ) || is_kiss_rust_cov_cache_dir(path)
}

pub(crate) fn is_kiss_rust_cov_cache_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("rust_llvm_cov_cache")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".kiss")
}

pub(crate) fn is_rust_cov_cache_input(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    {
        return true;
    }
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Cargo.toml" | "Cargo.lock" | "config.toml")
    ) || is_cargo_config_input_path(path)
        || is_rust_toolchain_input_path(path)
}

pub(crate) fn is_cargo_config_input_path(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some(".cargo")
        && matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("config" | "config.toml")
        )
}

pub(crate) fn is_rust_toolchain_input_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rust-toolchain"))
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

        let names: BTreeSet<_> = rust_cov_input_files(tmp.path())
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
        assert!(should_skip_rust_cov_dir(&tmp.path().join("target")));
        assert!(is_kiss_rust_cov_cache_dir(
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
