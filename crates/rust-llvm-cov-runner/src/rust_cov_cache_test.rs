use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use tempfile::TempDir;

use super::rust_cov_cache;
use crate::shared_input;
use super::{RustCovCacheEntry, RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};

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
        cache_status: RustCovCacheStatus::MissStored,
        stdout: Some(b"out".to_vec()),
        stderr: Some(b"err".to_vec()),
    }
}

#[test]
fn rust_cov_cache_round_trips_entries_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let entry = rust_cov_cache::RustCovCacheEntry::from(&outcome());

    rust_cov_cache::store_rust_cov_cache_entry(tmp.path(), "abc123", &entry).unwrap();
    let loaded = rust_cov_cache::load_rust_cov_cache_entry(tmp.path(), "abc123").unwrap();

    assert_eq!(loaded.selector, "smoke_sub");
    assert_eq!(loaded.status, TestStatus::Passed);
    assert_eq!(loaded.coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
    assert!(
        rust_cov_cache::rust_cov_cache_entry_path(tmp.path(), "abc123")
            .ends_with("entries/abc123.json")
    );
    assert!(rust_cov_cache::load_rust_cov_cache_entry(tmp.path(), "missing").is_none());
}

#[test]
fn rust_cov_cache_rejects_duplicate_temp_file_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("entry.tmp");

    let mut file = rust_cov_cache::create_new_cache_file(&path).unwrap();
    file.write_all(b"payload").unwrap();

    assert!(rust_cov_cache::create_new_cache_file(&path).is_err());
}

#[test]
fn rust_cov_cache_inputs_include_cargo_files_and_skip_generated_dirs() {
    let tmp = rust_cov_input_fixture();

    let names: BTreeSet<_> = shared_input::rust_cov_input_files(tmp.path())
        .unwrap()
        .into_iter()
        .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();

    assert!(names.contains(Path::new("Cargo.toml")));
    assert!(names.contains(Path::new("Cargo.lock")));
    assert!(names.contains(Path::new("rust-toolchain.toml")));
    assert!(names.contains(Path::new(".cargo/config.toml")));
    assert!(names.contains(Path::new("src/lib.rs")));
    assert!(names.contains(Path::new("src/fragment.inc")));
    assert!(!names.contains(Path::new("target/ignored.rs")));
    assert!(shared_input::should_skip_rust_cov_dir(
        &tmp.path().join("target")
    ));
    assert!(shared_input::is_kiss_rust_cov_cache_dir(
        &tmp.path().join(".kiss").join("rust_llvm_cov_cache")
    ));
}

fn rust_cov_input_fixture() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    create_rust_cov_input_dirs(tmp.path());
    write_rust_cov_input_files(tmp.path());
    tmp
}

fn create_rust_cov_input_dirs(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(root.join(".kiss").join("rust_llvm_cov_cache")).unwrap();
}

fn write_rust_cov_input_files(root: &Path) {
    fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(root.join("Cargo.lock"), "# lock\n").unwrap();
    fs::write(root.join("rust-toolchain.toml"), "[toolchain]\n").unwrap();
    fs::write(root.join(".cargo").join("config.toml"), "[build]\n").unwrap();
    fs::write(root.join("src").join("lib.rs"), "pub fn value() {}\n").unwrap();
    fs::write(
        root.join("src").join("fragment.inc"),
        "pub fn fragment() {}\n",
    )
    .unwrap();
    fs::write(root.join("target").join("ignored.rs"), "ignored\n").unwrap();
}

#[test]
fn rust_cov_cache_input_predicates_match_supported_files() {
    assert!(shared_input::is_rust_cov_cache_input(Path::new(
        "src/lib.rs"
    )));
    assert!(shared_input::is_rust_cov_cache_input(Path::new(
        "src/fragment.inc"
    )));
    assert!(shared_input::is_rust_cov_cache_input(Path::new(
        "Cargo.toml"
    )));
    assert!(shared_input::is_rust_cov_cache_input(Path::new(
        "Cargo.lock"
    )));
    assert!(shared_input::is_rust_cov_cache_input(Path::new(
        "rust-toolchain"
    )));
    assert!(shared_input::is_cargo_config_input_path(Path::new(
        ".cargo/config"
    )));
    assert!(shared_input::is_cargo_config_input_path(Path::new(
        ".cargo/config.toml"
    )));
    assert!(shared_input::is_rust_toolchain_input_path(Path::new(
        "rust-toolchain.toml"
    )));
    assert!(!shared_input::is_rust_cov_cache_input(Path::new(
        "README.md"
    )));
}

#[test]
fn rust_cov_cache_hash_and_suffix_helpers_are_stable_enough_for_cache_keys() {
    assert_ne!(rust_cov_cache::rust_cov_unique_suffix(), "");
    assert_eq!(
        rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"hello"),
        0xa430_d846_80aa_bd0b
    );
    assert_ne!(
        rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
        rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
    );
}

#[test]
fn repo_relative_and_generation_entry_fingerprint_helpers() {
    let repo = tempfile::tempdir().unwrap();
    let source = repo.path().join("src").join("lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "pub fn x() {}\n").unwrap();
    assert_eq!(
        rust_cov_cache::repo_relative_path(repo.path(), &source).as_deref(),
        Some("src/lib.rs")
    );
    assert_eq!(
        rust_cov_cache::repo_relative_coverage_file(repo.path(), &source.to_string_lossy())
            .as_deref(),
        Some("src/lib.rs")
    );
    let cache_root = repo.path().join(".kiss").join("rust_llvm_cov_cache");
    fs::create_dir_all(cache_root.join("entries")).unwrap();
    let entry = RustCovCacheEntry::from_outcome(&outcome(), "gen-a");
    rust_cov_cache::store_rust_cov_cache_entry(&cache_root, "abc123", &entry).unwrap();
    let fingerprint = rust_cov_cache::generation_entries_fingerprint(&cache_root, "gen-a").unwrap();
    assert!(!fingerprint.is_empty());
}

#[test]
fn store_rust_cov_cache_entry_source_does_not_zero_duration() {
    let src = include_str!("rust_cov_cache.rs");
    assert!(
        !src.contains("stable.duration = std::time::Duration::ZERO")
            && !src.contains("duration = std::time::Duration::ZERO"),
        "store must persist measured duration; do not reintroduce unconditional zeroing"
    );
}
