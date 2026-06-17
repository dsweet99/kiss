use super::{
    cache_has_coverage_failure, coverage_repo_root, item_path_buf, load_top_compatible_cache,
    load_top_only_cache, maybe_store_stats_top_cache, runtime_fingerprints_match,
    top_only_fingerprint, try_run_cached_stats_summary, try_run_cached_stats_top,
};
use crate::analyze_cache::test_helpers::{ScopedHome, empty_cache, empty_inputs};
use crate::analyze_cache::{fingerprint_for_check, store_full_cache_from_run};
use kiss::check_universe_cache::CachedCoverageItem;
use kiss::{Config, GateConfig};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn try_run_cached_stats_top_returns_none_on_cache_miss() {
    let _home = ScopedHome::new();
    let py = vec![PathBuf::from("/tmp/no_such_file.py")];
    let rs: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let got = try_run_cached_stats_top(&py, &rs, &py_cfg, &rs_cfg, &gate);
    assert!(got.is_none(), "expected cache miss for nonexistent paths");
}

#[test]
fn maybe_store_then_try_run_cached_stats_top_returns_coverage_map() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("a.py");
    std::fs::write(&py, "def alpha(): pass\ndef beta(): pass\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();

    let definitions = vec![
        CachedCoverageItem {
            file: py.to_string_lossy().to_string(),
            name: "alpha".into(),
            line: 1,
        },
        CachedCoverageItem {
            file: py.to_string_lossy().to_string(),
            name: "beta".into(),
            line: 2,
        },
    ];
    let unreferenced = vec![CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "alpha".into(),
        line: 1,
    }];
    maybe_store_stats_top_cache(
        &py_files,
        &rs_files,
        &py_cfg,
        &rs_cfg,
        &gate,
        definitions,
        unreferenced,
    );

    let map = try_run_cached_stats_top(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
        .expect("cache hit expected after store");
    assert_eq!(
        map.get(&py).copied(),
        Some(50),
        "1 of 2 defs unreferenced -> 50% coverage; got {map:?}"
    );
}

#[test]
fn private_helpers_round_trip() {
    let _home = ScopedHome::new();
    let item = CachedCoverageItem {
        file: "/tmp/example.py".into(),
        name: "alpha".into(),
        line: 7,
    };
    assert_eq!(item_path_buf(&item), PathBuf::from("/tmp/example.py"));

    let py: Vec<PathBuf> = Vec::new();
    let rs: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp = top_only_fingerprint(&py, &rs, &py_cfg, &rs_cfg, &gate);
    assert!(fp.starts_with("stats_top_"));
    assert!(load_top_only_cache(&py, &rs, &py_cfg, &rs_cfg, &gate).is_none());
    assert!(load_top_compatible_cache(&py, &rs, &py_cfg, &rs_cfg, &gate).is_none());
    assert!(try_run_cached_stats_summary(&py, &rs, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn maybe_store_stats_top_cache_does_not_overwrite_existing_cache() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("b.py");
    std::fs::write(&py, "def x(): pass\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();

    let initial_defs = vec![CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "x".into(),
        line: 1,
    }];
    maybe_store_stats_top_cache(
        &py_files,
        &rs_files,
        &py_cfg,
        &rs_cfg,
        &gate,
        initial_defs.clone(),
        Vec::new(),
    );
    let first = try_run_cached_stats_top(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
        .expect("cache hit expected");
    assert_eq!(first.get(&py).copied(), Some(100));

    maybe_store_stats_top_cache(
        &py_files,
        &rs_files,
        &py_cfg,
        &rs_cfg,
        &gate,
        initial_defs,
        vec![CachedCoverageItem {
            file: py.to_string_lossy().to_string(),
            name: "x".into(),
            line: 1,
        }],
    );
    let second = try_run_cached_stats_top(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
        .expect("cache still hits");
    assert_eq!(
        second.get(&py).copied(),
        Some(100),
        "second store should be a no-op; cache should still report initial 100%"
    );
}

#[test]
fn try_run_cached_stats_top_rejects_empty_coverage_cache() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("empty.py");
    std::fs::write(&py, "def x(): pass\n").unwrap();
    let py_files = vec![py];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();

    maybe_store_stats_top_cache(
        &py_files,
        &rs_files,
        &py_cfg,
        &rs_cfg,
        &gate,
        Vec::new(),
        Vec::new(),
    );

    assert!(try_run_cached_stats_top(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn maybe_store_stats_top_cache_rejects_runtime_failure_sentinels() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("sentinel.py");
    std::fs::write(&py, "def x(): pass\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();

    maybe_store_stats_top_cache(
        &py_files,
        &rs_files,
        &py_cfg,
        &rs_cfg,
        &gate,
        vec![CachedCoverageItem {
            file: py.to_string_lossy().to_string(),
            name: "rslip_refresh_failed".into(),
            line: 1,
        }],
        Vec::new(),
    );

    assert!(try_run_cached_stats_top(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn coverage_repo_root_climbs_to_project_marker() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let rs = src.join("lib.rs");
    std::fs::write(&rs, "pub fn f() -> i32 { 1 }\n").unwrap();

    assert_eq!(coverage_repo_root(&[], &[rs]), tmp.path());
}

#[test]
fn runtime_fingerprint_match_requires_both_backends() {
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("a.py");
    std::fs::write(&py, "def x(): pass\n").unwrap();
    let py_files = vec![py];
    let rs_files: Vec<PathBuf> = Vec::new();
    let repo_root = coverage_repo_root(&py_files, &rs_files);
    let mut cache = empty_cache("fp");
    cache.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(&repo_root);
    cache.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(&repo_root);

    assert!(runtime_fingerprints_match(&cache, &py_files, &rs_files));
    cache.rslip_fingerprint.push_str("-stale");
    assert!(!runtime_fingerprints_match(&cache, &py_files, &rs_files));
}

#[test]
fn cache_has_coverage_failure_detects_runtime_sentinels() {
    let mut cache = empty_cache("fp");
    cache.definitions.push(CachedCoverageItem {
        file: "a.py".into(),
        name: "llvm_cov_failed".into(),
        line: 1,
    });

    assert!(cache_has_coverage_failure(&cache));
}

#[test]
fn cached_stats_summary_requires_language_stats_for_cached_files() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("summary.py");
    std::fs::write(&py, "def x(): pass\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let repo_root = coverage_repo_root(&py_files, &rs_files);
    let mut inputs = empty_inputs(&fp);
    inputs.py_file_count = 1;
    inputs.py_paths = vec![py.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(&repo_root);
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(&repo_root);
    store_full_cache_from_run(inputs);

    assert!(try_run_cached_stats_summary(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn cached_stats_summary_requires_rust_stats_for_cached_rust_files() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let rs = tmp.path().join("lib.rs");
    std::fs::write(&rs, "pub fn x() -> i32 { 1 }\n").unwrap();
    let py_files: Vec<PathBuf> = Vec::new();
    let rs_files = vec![rs.clone()];
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let repo_root = coverage_repo_root(&py_files, &rs_files);
    let mut inputs = empty_inputs(&fp);
    inputs.rs_file_count = 1;
    inputs.rs_paths = vec![rs.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(&repo_root);
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(&repo_root);
    store_full_cache_from_run(inputs);

    assert!(try_run_cached_stats_summary(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn compatible_stats_top_cache_rejects_runtime_failure_sentinel() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let rs = tmp.path().join("lib.rs");
    std::fs::write(&rs, "pub fn x() -> i32 { 1 }\n").unwrap();
    let py_files: Vec<PathBuf> = Vec::new();
    let rs_files = vec![rs.clone()];
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let repo_root = coverage_repo_root(&py_files, &rs_files);
    let mut inputs = empty_inputs(&fp);
    inputs.rs_file_count = 1;
    inputs.rs_paths = vec![rs.to_string_lossy().to_string()];
    inputs.definitions = vec![CachedCoverageItem {
        file: rs.to_string_lossy().to_string(),
        name: "llvm_cov_failed".into(),
        line: 1,
    }];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(&repo_root);
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(&repo_root);
    store_full_cache_from_run(inputs);

    assert!(load_top_compatible_cache(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn coverage_repo_root_handles_empty_and_disjoint_relative_paths() {
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    assert_eq!(coverage_repo_root(&[], &[]), current);
    assert_eq!(
        coverage_repo_root(&[PathBuf::from("left.py")], &[PathBuf::from("right.rs")]),
        PathBuf::new()
    );
}
