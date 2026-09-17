use super::{
    SiblingGateResult, apply_time_gate_eval, evaluate_max_num_tests_gate,
    evaluate_time_gate_for_cov, finish_sibling_gates,
};
use crate::bin_cli::cov_cmd::{CovCommandArgs, CovFileSets};
use crate::test_runner::unit_test_timing::{RuntimeGateEval, RuntimeGateViolation};
use kiss::{Config, GateConfig, Language};
use std::path::{Path, PathBuf};

#[test]
fn finish_sibling_gates_exits_nonzero_when_either_fails() {
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: true,
            time_failed: false,
            max_num_tests_failed: false,
            orphan_failed: false,
        }),
        1
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: true,
            max_num_tests_failed: false,
            orphan_failed: false,
        }),
        1
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: false,
            max_num_tests_failed: true,
            orphan_failed: false,
        }),
        1
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: false,
            max_num_tests_failed: false,
            orphan_failed: false,
        }),
        0
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: false,
            max_num_tests_failed: false,
            orphan_failed: true,
        }),
        1
    );
}

#[test]
fn apply_time_gate_eval_variants() {
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Disabled));
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Passed));
    assert!(apply_time_gate_eval(&RuntimeGateEval::Incomplete));
    let viols = vec![RuntimeGateViolation {
        language: Language::Python,
        selector: "t.py::test_a".into(),
        seconds: 3.0,
        limit_seconds: 1.0,
    }];
    assert!(apply_time_gate_eval(&RuntimeGateEval::Failed(viols)));
}

#[test]
fn evaluate_time_gate_disabled_without_limits() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        max_unit_test_seconds: Vec::new(),
        ..GateConfig::default()
    };
    let args = CovCommandArgs {
        paths: &[],
        lang_filter: None,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: false,
        jobs: 1,
        allow_refresh: false,
        pytest_args: &[],
        language_tables: Default::default(),
    };
    let files = CovFileSets {
        py_files: vec![PathBuf::from("a.py")],
        rs_files: Vec::new(),
    };
    assert_eq!(
        evaluate_time_gate_for_cov(&args, Path::new("."), &files, &[]),
        RuntimeGateEval::Disabled
    );
}

#[test]
fn max_num_tests_gate_skipped_when_bypass() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        max_num_tests: 1,
        ..GateConfig::default()
    };
    let args = CovCommandArgs {
        paths: &[],
        lang_filter: None,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: true,
        ignore: &[],
        timing: false,
        jobs: 1,
        allow_refresh: false,
        pytest_args: &[],
        language_tables: Default::default(),
    };
    assert!(!evaluate_max_num_tests_gate(
        &args,
        Path::new("."),
        &CovFileSets {
            py_files: vec![PathBuf::from("a.py")],
            rs_files: Vec::new(),
        },
        &[],
    ));
}

#[test]
fn max_num_tests_gate_fails_closed_without_population() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        max_num_tests: 1,
        ..GateConfig::default()
    };
    let args = CovCommandArgs {
        paths: &[],
        lang_filter: None,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: false,
        jobs: 1,
        allow_refresh: false,
        pytest_args: &[],
        language_tables: Default::default(),
    };
    let tmp = tempfile::tempdir().unwrap();
    let files = CovFileSets {
        py_files: vec![tmp.path().join("lib.py")],
        rs_files: Vec::new(),
    };
    assert!(evaluate_max_num_tests_gate(&args, tmp.path(), &files, &[]));
}
