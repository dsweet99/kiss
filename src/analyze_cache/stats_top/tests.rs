use super::{
    item_path_buf, load_top_compatible_cache, load_top_only_cache, maybe_store_stats_top_cache,
    top_only_fingerprint, try_run_cached_stats_summary, try_run_cached_stats_top,
};
use crate::analyze_cache::{FullCacheInputs, store_full_cache_from_run, test_helpers::ScopedHome};
use kiss::check_universe_cache::CachedCoverageItem;
use kiss::{Config, GateConfig, MetricStats};
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
fn try_run_cached_stats_summary_requires_stats_for_present_languages() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("summary.py");
    std::fs::write(&py, "def alpha(): pass\n").unwrap();
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
        vec![CachedCoverageItem {
            file: py_files[0].to_string_lossy().to_string(),
            name: "alpha".into(),
            line: 1,
        }],
        Vec::new(),
    );

    assert!(try_run_cached_stats_summary(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn try_run_cached_stats_summary_requires_rust_stats_for_rust_files() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let rs = tmp.path().join("summary.rs");
    std::fs::write(&rs, "pub fn alpha() {}\n").unwrap();
    let py_files: Vec<PathBuf> = Vec::new();
    let rs_files = vec![rs];
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp =
        crate::analyze_cache::fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);

    store_full_cache_from_run(FullCacheInputs {
        fingerprint: fp,
        py_file_count: 0,
        rs_file_count: 1,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        coverage_violations: &[],
        runtime_coverage_identity: None,
        runtime_line_coverage: Vec::new(),
        py_graph: None,
        rs_graph: None,
        py_stats: None,
        rs_stats: None,
        focus_paths: Vec::new(),
        focus_restrict: false,
        py_paths: Vec::new(),
        rs_paths: rs_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        py_dups_all: &[],
        rs_dups_all: &[],
        definitions: Vec::new(),
        unreferenced: Vec::new(),
    });

    assert!(try_run_cached_stats_summary(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate).is_none());
}

#[test]
fn try_run_cached_stats_summary_returns_full_cache_when_stats_present() {
    let _home = ScopedHome::new();
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("summary_hit.py");
    std::fs::write(&py, "def alpha(): pass\n").unwrap();
    let py_files = vec![py];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp =
        crate::analyze_cache::fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let py_stats = MetricStats::default();

    store_full_cache_from_run(FullCacheInputs {
        fingerprint: fp.clone(),
        py_file_count: 1,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        coverage_violations: &[],
        runtime_coverage_identity: None,
        runtime_line_coverage: Vec::new(),
        py_graph: None,
        rs_graph: None,
        py_stats: Some(&py_stats),
        rs_stats: None,
        focus_paths: Vec::new(),
        focus_restrict: false,
        py_paths: py_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        rs_paths: Vec::new(),
        py_dups_all: &[],
        rs_dups_all: &[],
        definitions: Vec::new(),
        unreferenced: Vec::new(),
    });

    let got = try_run_cached_stats_summary(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
        .expect("summary cache hit");
    assert_eq!(got.fingerprint, fp);
    assert!(got.py_stats.is_some());
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
