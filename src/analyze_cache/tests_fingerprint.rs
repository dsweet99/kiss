use super::test_helpers::{ScopedHome, empty_inputs};
use super::*;
use crate::analyze::FocusFilter;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[test]
fn fingerprint_includes_python_annotations_per_function() {
    let gate = GateConfig::default();
    let rs = Config::rust_defaults();
    let base = Config::python_defaults();
    let mut other = base.clone();
    other.annotations_per_function = base.annotations_per_function.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &base, &rs, &gate),
        fingerprint_for_check(&[], &[], &other, &rs, &gate),
    );
}

#[test]
fn fingerprint_includes_python_returns_per_function() {
    let gate = GateConfig::default();
    let rs = Config::rust_defaults();
    let base = Config::python_defaults();
    let mut other = base.clone();
    other.returns_per_function = base.returns_per_function.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &base, &rs, &gate),
        fingerprint_for_check(&[], &[], &other, &rs, &gate),
    );
}

#[test]
fn fingerprint_includes_gate_test_coverage_threshold() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let g0 = GateConfig::default();
    let mut g1 = g0.clone();
    g1.test_coverage_threshold = g0.test_coverage_threshold.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &py, &rs, &g0),
        fingerprint_for_check(&[], &[], &py, &rs, &g1),
    );
}

#[test]
fn fingerprint_ignores_metadata_only_file_changes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("stable.py");
    fs::write(&path, "def stable():\n    return 1\n").unwrap();
    let py_files = vec![path.clone()];
    let before = fingerprint_for_check(
        &py_files,
        &[],
        &Config::python_defaults(),
        &Config::rust_defaults(),
        &GateConfig::default(),
    );

    let new_mtime = SystemTime::now() + Duration::from_secs(60);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(new_mtime)
        .unwrap();
    let after = fingerprint_for_check(
        &py_files,
        &[],
        &Config::python_defaults(),
        &Config::rust_defaults(),
        &GateConfig::default(),
    );

    assert_eq!(
        before, after,
        "metadata-only changes should not force a new full-check cache key"
    );
}

#[test]
fn fingerprint_covers_all_config_fields() {
    let field_count = std::mem::size_of::<Config>() / std::mem::size_of::<usize>();
    assert_eq!(
        field_count, 23,
        "Config field count changed; update mix_config_into_fingerprint and this test"
    );
    let Config {
        statements_per_function: _,
        methods_per_class: _,
        statements_per_file: _,
        lines_per_file: _,
        functions_per_file: _,
        arguments_positional: _,
        arguments_keyword_only: _,
        max_indentation_depth: _,
        interface_types_per_file: _,
        concrete_types_per_file: _,
        nested_function_depth: _,
        returns_per_function: _,
        return_values_per_function: _,
        branches_per_function: _,
        local_variables_per_function: _,
        imported_names_per_file: _,
        statements_per_try_block: _,
        boolean_parameters: _,
        annotations_per_function: _,
        calls_per_function: _,
        cycle_size: _,
        indirect_dependencies: _,
        dependency_depth: _,
    } = Config::python_defaults();
}

#[test]
fn rslip_database_fingerprint_changes_when_db_changes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    assert_eq!(
        kiss::rslip_bridge::rslip_database_fingerprint(root),
        "MISSING"
    );
    std::fs::create_dir_all(root.join(".kiss")).unwrap();
    std::fs::write(root.join(".kiss/rslip.json"), r#"{"schema_version":4}"#).unwrap();
    let first = kiss::rslip_bridge::rslip_database_fingerprint(root);
    std::fs::write(
        root.join(".kiss/rslip.json"),
        r#"{"schema_version":4,"tests":{}}"#,
    )
    .unwrap();
    let second = kiss::rslip_bridge::rslip_database_fingerprint(root);
    assert_ne!(first, second);
    assert_ne!(first, "MISSING");
}

#[test]
fn try_run_cached_all_rejects_stale_rslip_fingerprint() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("a.py");
    fs::write(&py, "def a():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.py_file_count = 1;
    inputs.py_paths = vec![py.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = "stale".to_string();
    store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    assert!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus).is_none(),
        "cache must miss when rslip fingerprint differs"
    );
}

#[test]
fn try_run_cached_all_replays_rslip_refresh_failure_snapshot() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("a.py");
    fs::write(&py, "def a():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.py_file_count = 1;
    inputs.py_paths = vec![py.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(tmp.path());
    inputs.definitions = vec![kiss::check_universe_cache::CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "rslip_refresh_failed".to_string(),
        line: 1,
    }];
    inputs.unreferenced = inputs.definitions.clone();
    store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    assert_eq!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus),
        Some(false),
        "fail-closed rslip snapshots should replay for check --all when fingerprints match"
    );
}

#[test]
fn try_run_cached_all_rejects_stale_rust_coverage_fingerprint() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let rs = tmp.path().join("lib.rs");
    fs::write(&rs, "pub fn a() -> usize { 1 }\n").unwrap();
    let py_files: Vec<PathBuf> = vec![];
    let rs_files = vec![rs.clone()];
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.rs_file_count = 1;
    inputs.rs_paths = vec![rs.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = "stale".to_string();
    store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    assert!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus).is_none(),
        "cache must miss when Rust coverage backend fingerprint differs"
    );
}

#[test]
fn try_run_cached_all_replays_llvm_cov_failure_snapshot() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let rs = tmp.path().join("lib.rs");
    fs::write(&rs, "pub fn a() -> usize { 1 }\n").unwrap();
    let py_files: Vec<PathBuf> = vec![];
    let rs_files = vec![rs.clone()];
    let py_cfg = Config::python_defaults();
    let rs_cfg = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_cfg, &rs_cfg, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.rs_file_count = 1;
    inputs.rs_paths = vec![rs.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(tmp.path());
    inputs.definitions = vec![kiss::check_universe_cache::CachedCoverageItem {
        file: rs.to_string_lossy().to_string(),
        name: "llvm_cov_failed".to_string(),
        line: 1,
    }];
    inputs.unreferenced = inputs.definitions.clone();
    store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: None,
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    assert_eq!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus),
        Some(false),
        "fail-closed llvm-cov snapshots should replay for check --all when fingerprints match"
    );
}
