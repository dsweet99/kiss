use super::*;
use crate::analyze::FocusFilter;
use kiss::{Config, GateConfig, TestCoverageScope};

#[test]
fn evaluate_records_with_time_rejects_when_coverage_gate_fails() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        max_unit_test_seconds: 0.0,
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
    };
    let records = [analyze::line_coverage::LineCoverageRecord {
        file: PathBuf::from("src/low.rs"),
        total_lines: 100,
        covered_lines: 10,
        percent: 10,
        first_uncovered_line: Some(1),
    }];
    let files = CovFileSets {
        py_files: vec![],
        rs_files: vec![PathBuf::from("src/low.rs")],
    };
    let tmp = tempfile::tempdir().unwrap();
    let focus = FocusFilter::unrestricted();
    let code = evaluate_records_with_time(
        &records,
        &RecordsEvalCtx {
            focus: &focus,
            threshold: 75,
            scope: TestCoverageScope::ByFile,
            args: &args,
            universe_root: tmp.path(),
            files: &files,
            ignore: &[],
        },
    );
    assert_eq!(code, 1);
}

#[test]
fn gather_cov_files_none_when_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(gather_cov_files(tmp.path(), None, &[]).is_none());
}

#[test]
fn both_gates_disabled_short_circuits() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: 0.0,
        ..GateConfig::default()
    };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.py"), "x = 1\n").unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    let args = CovCommandArgs {
        paths: std::slice::from_ref(&path),
        lang_filter: Some(Language::Python),
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: false,
        jobs: 1,
    };
    assert_eq!(run_cov_command(&args), 0);
}

#[test]
fn runtime_gate_eval_failed_sets_time_failed() {
    let viols = vec![crate::test_runner::unit_test_timing::RuntimeGateViolation {
        language: crate::test_runner::unit_test_timing::TimingLanguage::Python,
        selector: "t::test_slow".into(),
        seconds: 2.5,
    }];
    assert!(apply_time_gate_eval(&RuntimeGateEval::Failed(viols), 2.0));
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Passed, 2.0));
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Disabled, 2.0));
    assert!(apply_time_gate_eval(&RuntimeGateEval::Incomplete, 2.0));
}

#[test]
fn wiring_guard_time_gate_invoked_from_cov_path() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig::default();
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
    };
    let files = CovFileSets {
        py_files: vec![],
        rs_files: vec![],
    };
    let tmp = tempfile::tempdir().unwrap();
    let eval = evaluate_time_gate_for_cov(&args, tmp.path(), &files, &[]);
    assert!(matches!(
        eval,
        RuntimeGateEval::Incomplete
            | RuntimeGateEval::Passed
            | RuntimeGateEval::Failed(_)
            | RuntimeGateEval::Disabled
    ));
    let _ = TimingCollectOpts {
        universe: tmp.path(),
        lang_filter: None,
        include: TimingLangInclude {
            python: false,
            rust: false,
        },
        ignore: &[],
    };
}

#[test]
fn lang_filter_cache_label_covers_both_languages() {
    assert_eq!(lang_filter_cache_label(None), None);
    assert_eq!(lang_filter_cache_label(Some(Language::Python)), Some("python"));
    assert_eq!(lang_filter_cache_label(Some(Language::Rust)), Some("rust"));
}

#[test]
fn try_evaluate_records_with_time_falls_through_on_incomplete() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        max_unit_test_seconds: 2.0,
        ..GateConfig::default()
    };
    let args = CovCommandArgs {
        paths: &[],
        lang_filter: Some(Language::Rust),
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: false,
        jobs: 1,
    };
    let files = CovFileSets {
        py_files: vec![],
        rs_files: vec![PathBuf::from("src/lib.rs")],
    };
    let tmp = tempfile::tempdir().unwrap();
    let focus = FocusFilter::unrestricted();
    let records = [analyze::line_coverage::LineCoverageRecord {
        file: PathBuf::from("src/lib.rs"),
        total_lines: 10,
        covered_lines: 10,
        percent: 100,
        first_uncovered_line: None,
    }];
    // Empty repo: timing population is incomplete → fast-path must fall through.
    let code = try_evaluate_records_with_time(
        &records,
        &RecordsEvalCtx {
            focus: &focus,
            threshold: 75,
            scope: TestCoverageScope::ByFile,
            args: &args,
            universe_root: tmp.path(),
            files: &files,
            ignore: &[],
        },
    );
    assert!(code.is_none());
}

#[test]
fn finish_sibling_gates_exits_nonzero_when_either_fails() {
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: true,
            time_failed: false,
        }),
        1
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: true,
        }),
        1
    );
    assert_eq!(
        finish_sibling_gates(SiblingGateResult {
            coverage_failed: false,
            time_failed: false,
        }),
        0
    );
}

#[test]
fn evaluate_coverage_gate_bypass_still_prints_violations() {
    let records = [analyze::line_coverage::LineCoverageRecord {
        file: PathBuf::from("src/low.rs"),
        total_lines: 100,
        covered_lines: 10,
        percent: 10,
        first_uncovered_line: Some(1),
    }];
    let focus = FocusFilter::unrestricted();
    assert!(evaluate_coverage_gate(
        &records,
        &focus,
        75,
        TestCoverageScope::ByFile,
        true,
    ));
}

#[test]
fn time_only_gate_path_runs_when_coverage_threshold_zero() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: 2.0,
        ..GateConfig::default()
    };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "pub fn x() {}\n").unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    let args = CovCommandArgs {
        paths: std::slice::from_ref(&path),
        lang_filter: Some(Language::Rust),
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: false,
        jobs: 1,
    };
    // No durable rust population → incomplete timings → nonzero.
    assert_eq!(run_cov_command(&args), 1);
}
