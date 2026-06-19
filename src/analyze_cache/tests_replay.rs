use super::all_replay::store_all_replay_cache;
use super::test_helpers::{ScopedHome, empty_cache, empty_inputs};
use super::*;
use crate::analyze::FocusFilter;
use kiss::check_cache::CachedViolation;
use kiss::check_universe_cache::CachedCoverageItem;
use std::path::PathBuf;

#[test]
fn try_run_cached_gate_failure_replays_without_full_cache() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("uncovered.py");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: false,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_config, &rs_config, &gate);
    let violations = vec![kiss::Violation {
        file: py,
        line: 0,
        unit_name: "file".to_string(),
        metric: "test_coverage".to_string(),
        value: 0,
        threshold: gate.test_coverage_threshold,
        message: "0% covered. Add test coverage for this source file.".to_string(),
        suggestion: String::new(),
    }];
    store_gate_failure_replay_cache(fp, &opts, &py_files, &rs_files, &focus, &violations);

    assert_eq!(
        try_run_cached_gate_failure(&opts, &py_files, &rs_files, &focus),
        Some(false)
    );
}

#[test]
fn cached_gate_failure_replay_preserves_suggestions() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("uncovered.py");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: false,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_config, &rs_config, &gate);
    let violations = vec![kiss::Violation {
        file: py,
        line: 1,
        unit_name: "uncovered".to_string(),
        metric: "test_coverage".to_string(),
        value: 0,
        threshold: gate.test_coverage_threshold,
        message: "0% covered.".to_string(),
        suggestion: "Add a direct test.".to_string(),
    }];
    store_gate_failure_replay_cache(fp, &opts, &py_files, &rs_files, &focus, &violations);

    assert_eq!(
        try_run_cached_gate_failure(&opts, &py_files, &rs_files, &focus),
        Some(false),
        "gate-failure replay should preserve non-empty suggestions"
    );
}

#[test]
fn compact_all_replay_serves_check_all_without_full_cache() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("uncovered.py");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_config, &rs_config, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.py_file_count = 1;
    inputs.py_paths = vec![py.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(tmp.path());
    inputs.definitions = vec![CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "uncovered".into(),
        line: 1,
    }];
    inputs.unreferenced = inputs.definitions.clone();
    let full = store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };
    store_all_replay_cache(&fp, &opts, &focus, &full);
    std::fs::remove_file(super::path_helpers::cache_path_full(&fp)).unwrap();

    assert_eq!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus),
        Some(false),
        "compact --all replay should not need the full raw coverage cache"
    );
}

#[test]
fn compact_all_replay_rejects_changed_source() {
    let _home = ScopedHome::new();
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("uncovered.py");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    let py_files = vec![py.clone()];
    let rs_files: Vec<PathBuf> = vec![];
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let fp = fingerprint_for_check(&py_files, &rs_files, &py_config, &rs_config, &gate);
    let mut inputs = empty_inputs(&fp);
    inputs.py_file_count = 1;
    inputs.py_paths = vec![py.to_string_lossy().to_string()];
    inputs.rslip_fingerprint = kiss::rslip_bridge::rslip_database_fingerprint(tmp.path());
    inputs.rust_coverage_fingerprint = kiss::rust_llvm_cov::backend_fingerprint(tmp.path());
    inputs.definitions = vec![CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "uncovered".into(),
        line: 1,
    }];
    inputs.unreferenced = inputs.definitions.clone();
    let full = store_full_cache_from_run(inputs);
    let focus = FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };
    store_all_replay_cache(&fp, &opts, &focus, &full);
    std::fs::remove_file(super::path_helpers::cache_path_full(&fp)).unwrap();
    std::fs::write(&py, "def uncovered():\n    return 2\n").unwrap();

    assert!(
        try_run_cached_all(&opts, &py_files, &rs_files, &focus).is_none(),
        "compact --all replay must reject changed source content"
    );
}

#[test]
fn cached_bypass_emits_file_level_coverage_from_raw_lists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("uncovered.py");
    std::fs::write(&py, "def uncovered():\n    return 1\n").unwrap();
    let mut cache = empty_cache("bypass_file_level");
    cache.py_file_count = 1;
    cache.py_paths = vec![py.to_string_lossy().to_string()];
    cache.definitions = vec![CachedCoverageItem {
        file: py.to_string_lossy().to_string(),
        name: "line_1".into(),
        line: 1,
    }];
    cache.unreferenced = cache.definitions.clone();
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };

    assert!(
        !super::emit::emit_cached_bypass(cache, &opts, &FocusFilter::unrestricted()),
        "cached --all replay should report the uncovered file"
    );
}

#[test]
fn cached_bypass_reports_base_violation_in_focus() {
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py = tmp.path().join("module.py");
    std::fs::write(&py, "def f():\n    return 1\n").unwrap();
    let mut cache = empty_cache("bypass_base");
    cache.py_file_count = 1;
    cache.py_paths = vec![py.to_string_lossy().to_string()];
    cache.base_violations.push(CachedViolation {
        file: py.to_string_lossy().to_string(),
        line: 1,
        unit_name: "f".into(),
        metric: "statements_per_function".into(),
        value: 2,
        threshold: 1,
        message: "too many statements".into(),
        suggestion: "Split it.".into(),
    });
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };

    assert!(
        !super::emit::emit_cached_bypass(cache, &opts, &FocusFilter::unrestricted()),
        "cached --all replay should report focused base violations"
    );
}

#[test]
fn cached_bypass_clean_cache_passes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let universe = tmp.path().to_string_lossy().to_string();
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate = GateConfig::default();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &universe,
        focus_paths: std::slice::from_ref(&universe),
        py_config: &py_config,
        rs_config: &rs_config,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
        collect_stats: false,
    };

    assert!(
        super::emit::emit_cached_bypass(
            empty_cache("bypass_clean"),
            &opts,
            &FocusFilter::unrestricted()
        ),
        "cached --all replay should pass when no cached violations exist"
    );
}
