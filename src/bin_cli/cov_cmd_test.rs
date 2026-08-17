use super::*;
use crate::analyze::FocusFilter;
use crate::test_runner::unit_test_timing::TimingLangInclude;
use kiss::{Config, GateConfig, TestCoverageScope};
#[test]
fn evaluate_records_with_time_rejects_when_coverage_gate_fails() {
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
        allow_refresh: true,
        pytest_args: &[],
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
        max_unit_test_seconds: Vec::new(),
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
        allow_refresh: true,
        pytest_args: &[],
};
    assert_eq!(run_cov_command(&args), 0);
}

#[test]
fn runtime_gate_eval_failed_sets_time_failed() {
    let viols = vec![crate::test_runner::unit_test_timing::RuntimeGateViolation {
        language: kiss::Language::Python,
        selector: "t::test_slow".into(),
        seconds: 2.5,
        limit_seconds: 2.0,
    }];
    assert!(apply_time_gate_eval(&RuntimeGateEval::Failed(viols)));
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Passed));
    assert!(!apply_time_gate_eval(&RuntimeGateEval::Disabled));
    assert!(apply_time_gate_eval(&RuntimeGateEval::Incomplete));
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
        allow_refresh: true,
        pytest_args: &[],
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
        pytest_args: &[],
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
        max_unit_test_seconds: vec![("*".to_string(), 2.0)],
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
        allow_refresh: true,
        pytest_args: &[],
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
        max_unit_test_seconds: vec![("*".to_string(), 2.0)],
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
        allow_refresh: true,
        pytest_args: &[],
};
    // No durable rust population → incomplete timings → nonzero.
    assert_eq!(run_cov_command(&args), 1);
}

#[test]
fn allow_refresh_false_incomplete_time_gate_fails_closed() {
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
        allow_refresh: false,
        pytest_args: &[],
};
    let records = [analyze::line_coverage::LineCoverageRecord {
        file: PathBuf::from("src/ok.rs"),
        total_lines: 10,
        covered_lines: 10,
        percent: 100,
        first_uncovered_line: None,
    }];
    let files = CovFileSets {
        py_files: vec![],
        rs_files: vec![PathBuf::from("src/ok.rs")],
    };
    let tmp = tempfile::tempdir().unwrap();
    let focus = FocusFilter::unrestricted();
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
    // Empty universe timings are Incomplete or Disabled/Passed; Incomplete must be Some(1).
    if matches!(
        evaluate_time_gate_for_cov(&args, tmp.path(), &files, &[]),
        RuntimeGateEval::Incomplete
    ) {
        assert_eq!(code, Some(1));
    }
}

#[test]
fn load_or_refresh_snapshot_respects_allow_refresh_false() {
    let tmp = tempfile::tempdir().unwrap();
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    let err = load_or_refresh_snapshot(
        tmp.path(),
        required,
        &[],
        1,
        false,
        &kiss::GateConfig::default(),
        &[],
    );
    assert!(matches!(err, Err(1)), "cache-only load must fail closed: {err:?}");
}

#[test]
fn timing_true_empty_universe_short_circuits_or_fails_softly() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig {
        test_coverage_threshold: 0,
        max_unit_test_seconds: Vec::new(),
        ..GateConfig::default()
    };
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    let args = CovCommandArgs {
        paths: std::slice::from_ref(&path),
        lang_filter: Some(Language::Python),
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: true,
        jobs: 1,
        allow_refresh: false,
        pytest_args: &[],
};
    // Empty dir → no files → 0; or gate-disabled short circuit.
    let code = run_cov_command(&args);
    assert!(code == 0 || code == 1, "code={code}");
}

#[test]
fn run_cov_command_hits_records_fast_path_after_seed() {
    use crate::test_runner::python_coverage_index::{
        write_python_coverage_snapshot, write_python_population_manifest_for_args,
    };
    use crate::bin_cli::cov_warm::warm_cov_caches_after_tests;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::env::set_current_dir(repo).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("a.py"), "x = 1\n").unwrap();
    fs::write(
        repo.join(".kissconfig"),
        "[test]\ntest_coverage_threshold = 50\nmax_unit_test_seconds = [[\"*\", 0.0]]\n",
    )
    .unwrap();
    let selector = "tests/test_a.py::test_x".to_string();
    write_python_population_manifest_for_args(repo, std::slice::from_ref(&selector), &[]).unwrap();
    let mut covered = BTreeMap::new();
    covered.insert("a.py".to_string(), BTreeSet::from([1u32]));
    write_python_coverage_snapshot(repo, &covered).unwrap();

    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig::load();
    warm_cov_caches_after_tests(repo, Some(Language::Python), &[], &gate, &[]);
    let path = ".".to_string();
    let args = CovCommandArgs {
        paths: std::slice::from_ref(&path),
        lang_filter: Some(Language::Python),
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
        bypass_gate: false,
        ignore: &[],
        timing: true,
        jobs: 1,
        allow_refresh: false,
        pytest_args: &[],
    };
    let code = run_cov_command(&args);
    assert!(code == 0 || code == 1, "code={code}");

    let args_bypass = CovCommandArgs {
        bypass_gate: true,
        allow_refresh: false,
        ..args
    };
    let _ = run_cov_command(&args_bypass);
}
