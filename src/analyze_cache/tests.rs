use super::path_helpers::load_full_cache;
use super::test_helpers::{ScopedHome, empty_cache, empty_inputs};
use super::*;
use crate::analyze::FocusFilter;
use crate::analyze::evaluate_cached_gate;
use kiss::check_cache::CachedViolation;
use kiss::check_universe_cache::{CachedCoverageItem, CachedFileCoverage};
use std::path::PathBuf;

#[test]
fn cached_coverage_viols_skips_test_files_on_replay() {
    let file = PathBuf::from("/tmp/repo/tests/test_lib.py");
    let cached = CachedViolation::from(&coverage_violation(file.clone(), "test_lib".into(), 1, 0));
    let mut cache = empty_cache("test_file_skip");
    cache.coverage_violations = vec![cached];
    let focus = FocusFilter::restricting([file.clone()].into_iter().collect());
    assert!(cached_coverage_viols(&cache, &focus).is_empty());
}

#[test]
fn cached_coverage_viols_replays_weighted_overlay_pct() {
    let file = PathBuf::from("src/sparse_module.py");
    let mut cache = empty_cache("weighted_overlay_replay");
    let item = CachedCoverageItem {
        file: file.to_string_lossy().to_string(),
        name: "fn_a".into(),
        line: 1,
    };
    cache.definitions.push(item.clone());
    cache.unreferenced.push(item);
    cache.weighted_file_pcts.push(CachedFileCoverage {
        file: file.to_string_lossy().to_string(),
        pct: 17,
    });
    let focus = FocusFilter::unrestricted();
    let viols = cached_coverage_viols(&cache, &focus);
    assert_eq!(viols.len(), 1);
    assert_eq!(viols[0].metric, "test_coverage");
    assert!(viols[0].message.contains("17% covered"));
}

#[test]
fn cached_gated_replay_matches_live_gate_without_weighted_overlay() {
    let file = PathBuf::from("src/module.rs");
    let mut cache = empty_cache("weighted_gate_replay");
    cache.definitions = vec![
        CachedCoverageItem {
            file: file.to_string_lossy().to_string(),
            name: "large_covered".into(),
            line: 1,
        },
        CachedCoverageItem {
            file: file.to_string_lossy().to_string(),
            name: "small_missed".into(),
            line: 20,
        },
    ];
    cache.unreferenced = vec![CachedCoverageItem {
        file: file.to_string_lossy().to_string(),
        name: "small_missed".into(),
        line: 20,
    }];
    cache.weighted_file_pcts = vec![CachedFileCoverage {
        file: file.to_string_lossy().to_string(),
        pct: 91,
    }];
    let focus = FocusFilter::restricting(std::iter::once(file).collect());
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate_config = GateConfig {
        test_coverage_threshold: 90,
        ..GateConfig::default()
    };
    let opts = crate::analyze::AnalyzeOptions {
        universe: ".",
        focus_paths: &[],
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &gate_config,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };

    let weighted = weighted_file_pct_map(&cache.weighted_file_pcts);
    assert!(
        evaluate_cached_gate(
            &cache.definitions,
            &cache.unreferenced,
            &focus,
            90,
            Some(&weighted),
        )
        .is_none(),
        "weighted overlay can still raise pct when passed explicitly"
    );
    assert!(
        !super::emit::emit_cached_gated(cache, &opts, &focus),
        "cached gated replay must match live evaluate_gate (no weighted overlay)"
    );
}

#[test]
fn fingerprint_path_duplicates_and_coverage_helpers() {
    let fp = fingerprint_for_check(
        &[],
        &[],
        &Config::python_defaults(),
        &Config::rust_defaults(),
        &GateConfig::default(),
    );
    assert!(!fp.is_empty());

    let v = coverage_violation(PathBuf::from("test.py"), "foo".into(), 1, 50);
    assert_eq!(v.metric, "test_coverage");
    assert!(v.message.contains("50%"));
    assert_eq!(graph_counts(None, None), (0, 0));

    cache_path_full("deadbeef");
    assert!(load_full_cache("deadbeef").is_none());

    let focus = FocusFilter::unrestricted();
    let (_viols, py_dups, rs_dups, cache) =
        cached_duplicates(empty_cache("deadbeef"), &GateConfig::default(), &focus);
    assert!(py_dups.is_empty() && rs_dups.is_empty());
    assert!(cached_coverage_viols(&cache, &focus).is_empty());
}

#[test]
fn fnv1a64_properties() {
    let h0 = 0xcbf2_9ce4_8422_2325_u64;
    assert_eq!(fnv1a64(h0, b""), h0);
    assert_eq!(fnv1a64(h0, b"hello"), fnv1a64(h0, b"hello"));
    assert_ne!(fnv1a64(h0, b"hello"), fnv1a64(h0, b"world"));
}

#[test]
fn try_run_cached_all_replays_default_coverage_gate_failure() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let rs = tmp.path().join("lib.rs");
    std::fs::write(&rs, "pub fn uncovered() -> i32 { 1 }\n").unwrap();
    let py_files: Vec<PathBuf> = vec![];
    let rs_files = vec![rs.clone()];
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_config, &rs_config, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.rs_file_count = 1;
    inputs.rs_paths = vec![rs.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(tmp.path());
    inputs.definitions = vec![CachedCoverageItem {
        file: rs.to_string_lossy().to_string(),
        name: "uncovered".into(),
        line: 1,
    }];
    inputs.unreferenced = inputs.definitions.clone();
    store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Rust),
        bypass_gate: false,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };
    assert_eq!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus),
        Some(false),
        "valid cached default coverage-gate failures should replay without rerunning coverage"
    );
}

#[test]
fn full_cache_inputs_and_store() {
    let _home = ScopedHome::new();
    let mut inputs = empty_inputs("test_fp_persist");
    inputs.py_file_count = 1;
    assert_eq!(inputs.py_file_count, 1);
    store_full_cache_from_run(inputs);
    let loaded = load_full_cache("test_fp_persist");
    assert_eq!(
        loaded.as_ref().map(|c| c.fingerprint.as_str()),
        Some("test_fp_persist")
    );
    assert_eq!(loaded.map(|c| c.py_file_count), Some(1));
}
