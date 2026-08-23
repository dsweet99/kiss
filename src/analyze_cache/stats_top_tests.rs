use super::super::content_digest::content_digests_for_paths;
use super::try_run_cached_stats_summary;
use crate::analyze_cache::{FullCacheInputs, store_full_cache_from_run};
use kiss::{Config, GateConfig, MetricStats};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn try_run_cached_stats_summary_requires_stats_for_present_languages() {
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("summary.py");
    std::fs::write(&py, "def alpha(): pass\n").unwrap();
    let py_files = vec![py];
    let rs_files: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    let fp =
        crate::analyze_cache::fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let universe = tmp.path().to_str().unwrap();

    store_full_cache_from_run(FullCacheInputs {
        repo_root: tmp.path().to_path_buf(),
        fingerprint: fp,
        py_file_count: 1,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        py_graph: None,
        rs_graph: None,
        py_stats: None,
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
        file_content_digests: content_digests_for_paths(&py_files),
    });

    assert!(
        try_run_cached_stats_summary(universe, &py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
            .is_none()
    );
}

#[test]
fn try_run_cached_stats_summary_requires_rust_stats_for_rust_files() {
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
    let universe = tmp.path().to_str().unwrap();

    store_full_cache_from_run(FullCacheInputs {
        repo_root: tmp.path().to_path_buf(),
        fingerprint: fp,
        py_file_count: 0,
        rs_file_count: 1,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
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
        file_content_digests: content_digests_for_paths(&rs_files),
    });

    assert!(
        try_run_cached_stats_summary(universe, &py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
            .is_none()
    );
}

#[test]
fn try_run_cached_stats_summary_returns_full_cache_when_stats_present() {
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
    let universe = tmp.path().to_str().unwrap();

    store_full_cache_from_run(FullCacheInputs {
        repo_root: tmp.path().to_path_buf(),
        fingerprint: fp.clone(),
        py_file_count: 1,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
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
        file_content_digests: content_digests_for_paths(&py_files),
    });

    let got = try_run_cached_stats_summary(universe, &py_files, &rs_files, &py_cfg, &rs_cfg, &gate)
        .expect("expected cache hit with stats");
    assert_eq!(got.fingerprint, fp);
}

#[test]
fn try_run_cached_stats_summary_misses_when_empty() {
    let tmp = TempDir::new().unwrap();
    let py: Vec<PathBuf> = Vec::new();
    let rs: Vec<PathBuf> = Vec::new();
    let py_cfg = Config::default();
    let rs_cfg = Config::default();
    let gate = GateConfig::default();
    assert!(
        try_run_cached_stats_summary(
            tmp.path().to_str().unwrap(),
            &py,
            &rs,
            &py_cfg,
            &rs_cfg,
            &gate
        )
        .is_none()
    );
}
