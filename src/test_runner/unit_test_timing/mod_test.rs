use super::*;
use kiss::Language;
use std::time::Duration;

fn rules(secs: f64) -> Vec<(String, f64)> {
    vec![("*".to_string(), secs)]
}

#[test]
fn evaluate_runtime_gate_threshold_semantics() {
    let timings = TimingPopulation::Complete(vec![UnitTestTiming {
        language: Language::Python,
        selector: "t::slow".into(),
        duration: Duration::from_millis(1500),
    }]);
    assert!(matches!(
        evaluate_runtime_gate(&timings, &[]),
        RuntimeGateEval::Disabled
    ));
    match evaluate_runtime_gate(&timings, &rules(1.0)) {
        RuntimeGateEval::Failed(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].selector, "t::slow");
            assert!((v[0].limit_seconds - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(matches!(
        evaluate_runtime_gate(&timings, &rules(2.0)),
        RuntimeGateEval::Passed
    ));

    let exact = TimingPopulation::Complete(vec![UnitTestTiming {
        language: Language::Rust,
        selector: "exact".into(),
        duration: Duration::from_secs(2),
    }]);
    assert!(matches!(
        evaluate_runtime_gate(&exact, &rules(2.0)),
        RuntimeGateEval::Failed(_)
    ));
}

#[test]
fn incomplete_population_fail_closed_when_enabled() {
    assert!(matches!(
        evaluate_runtime_gate(&TimingPopulation::Incomplete, &rules(2.0)),
        RuntimeGateEval::Incomplete
    ));
}

#[test]
fn runtime_gate_failure_lines_count_only() {
    let lines = runtime_gate_failure_lines(&[
        RuntimeGateViolation {
            language: Language::Rust,
            selector: "crate::b".into(),
            seconds: 3.0,
            limit_seconds: 2.0,
        },
        RuntimeGateViolation {
            language: Language::Python,
            selector: "tests/test_x.py::test_y".into(),
            seconds: 2.41,
            limit_seconds: 2.0,
        },
    ]);
    assert_eq!(
        lines,
        ["VIOLATION:max_unit_test_seconds: 2 test(s) exceeded path-pattern time limits"]
    );
}

#[test]
fn path_pattern_limits_differ_per_selector() {
    let rules = vec![("tests/fast".to_string(), 2.0), ("*".to_string(), 0.0)];
    let timings = TimingPopulation::Complete(vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fast/a.py::t".into(),
            duration: Duration::from_millis(500),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/other/b.py::t".into(),
            duration: Duration::from_millis(1),
        },
    ]);
    match evaluate_runtime_gate(&timings, &rules) {
        RuntimeGateEval::Failed(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].selector, "tests/other/b.py::t");
            assert!((v[0].limit_seconds - 0.0).abs() < f64::EPSILON);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn rust_report_id_selectors_match_rust_path_pattern() {
    let rules = vec![("rust".to_string(), 10.0), ("*".to_string(), 0.0)];
    let timings = TimingPopulation::Complete(vec![
        UnitTestTiming {
            language: Language::Rust,
            selector: "rust/sameq_style/src/lib.rs::test_check_clean".into(),
            duration: Duration::from_millis(500),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "bare_logical_name".into(),
            duration: Duration::from_millis(1),
        },
    ]);
    match evaluate_runtime_gate(&timings, &rules) {
        RuntimeGateEval::Failed(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].selector, "bare_logical_name");
            assert!((v[0].limit_seconds - 0.0).abs() < f64::EPSILON);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn empty_lang_selection_is_complete_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = collect_current_unit_test_timings(TimingCollectOpts {
        universe: tmp.path(),
        lang_filter: None,
        include: TimingLangInclude {
            python: false,
            rust: false,
        },
        ignore: &[],
        pytest_args: &[],
    });
    assert_eq!(pop, TimingPopulation::Complete(vec![]));
}

#[test]
fn ignore_prefixes_drop_matching_path_selectors() {
    let timings = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fast/test_a.py::t".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/test_b.py::t".into(),
            duration: Duration::from_millis(50),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "crate::mod::tests::t".into(),
            duration: Duration::from_millis(20),
        },
    ];
    let filtered = filter_timings_by_ignore(timings, &["tests/slow".into()]);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].selector, "tests/fast/test_a.py::t");
    assert_eq!(filtered[1].selector, "crate::mod::tests::t");

    let mixed = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fake_python/test_x.py::test_x".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/test_app.py::test_app".into(),
            duration: Duration::from_millis(10),
        },
    ];
    let filtered = filter_timings_by_ignore(mixed, &["fake_".into()]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].selector, "tests/test_app.py::test_app");

    let rust_logical = vec![
        UnitTestTiming {
            language: Language::Rust,
            selector: "tests::unit_ok".into(),
            duration: Duration::from_secs(2),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "src/lib.rs::unit_ok".into(),
            duration: Duration::from_secs(2),
        },
    ];
    let filtered = filter_timings_by_ignore(rust_logical, &["tests".into()]);
    assert_eq!(filtered.len(), 2);
    let filtered = filter_timings_by_ignore(
        vec![UnitTestTiming {
            language: Language::Rust,
            selector: "src/lib.rs::unit_ok".into(),
            duration: Duration::from_secs(2),
        }],
        &["src".into()],
    );
    assert!(filtered.is_empty());
}

#[test]
fn path_max_gate_reports_slowest_selector_per_path() {
    let path_maxes = vec![
        crate::test_runner::python_coverage_index::generation::PathMaxDuration {
            path: "tests/fast/a.py".into(),
            max_duration_ns: 500_000_000,
            example_selector: "tests/fast/a.py::t".into(),
        },
        crate::test_runner::python_coverage_index::generation::PathMaxDuration {
            path: "tests/other/b.py".into(),
            max_duration_ns: 1_000_000,
            example_selector: "tests/other/b.py::t".into(),
        },
    ];
    let rules = vec![("tests/fast".to_string(), 2.0), ("*".to_string(), 0.0)];
    let viols = evaluate_path_max_runtime_violations(&path_maxes, &rules, &[]);
    assert_eq!(viols.len(), 1);
    assert_eq!(viols[0].selector, "tests/other/b.py::t");
}

#[test]
fn path_max_gate_respects_ignore_prefixes() {
    let path_maxes = vec![
        crate::test_runner::python_coverage_index::generation::PathMaxDuration {
            path: "tests/slow/a.py".into(),
            max_duration_ns: 50_000_000_000,
            example_selector: "tests/slow/a.py::t".into(),
        },
    ];
    let rules = vec![("*".to_string(), 1.0)];
    let viols = evaluate_path_max_runtime_violations(&path_maxes, &rules, &["tests/slow".into()]);
    assert!(viols.is_empty());
}

#[test]
fn evaluate_cov_time_gate_passes_when_workspace_selectors_are_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::write(repo.join("app.py"), "x = 1\n").unwrap();
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        repo,
        &[],
        &[],
        &[],
        &[],
    )
    .expect("store empty python selectors");
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &[("*".to_string(), 1.0)],
        timing: false,
        pytest_args: &[],
    });
    assert_eq!(eval, RuntimeGateEval::Passed);
}

#[test]
fn evaluate_cov_time_gate_does_not_skip_rust_mod_tests_for_ignore_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src").join("lib.rs"), "pub fn f() {}\n").unwrap();
    crate::test_runner::workspace_selector_cache::clear_rust_selector_memo_for_tests();
    let ignore = ["tests".to_string()];
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        repo,
        &ignore,
        &[],
        &["tests::unit_ok".into()],
        &[],
    )
    .expect("store rust logical selectors");
    crate::test_runner::workspace_selector_cache::clear_rust_selector_memo_for_tests();
    clear_rust_duration_pairs_memo();
    set_pairs_for_tests(
        repo,
        vec![("tests::unit_ok".into(), Duration::from_secs(2))],
    );
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Rust),
        include: TimingLangInclude {
            python: false,
            rust: true,
        },
        ignore: &ignore,
        limits: &[("*".to_string(), 0.5)],
        timing: false,
        pytest_args: &[],
    });
    match eval {
        RuntimeGateEval::Failed(viols) => {
            assert_eq!(viols.len(), 1);
            assert_eq!(viols[0].selector, "tests::unit_ok");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn evaluate_cov_time_gate_empty_selectors_do_not_hide_slow_population() {
    use crate::test_runner::python_coverage_index::{
        python_coverage_cache_root, read_python_population_manifest, write_population_durations,
        write_python_population_manifest_for_args,
    };
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::write(repo.join("app.py"), "x = 1\n").unwrap();
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(
        repo.join("tests").join("test_slow.py"),
        "def test_slow():\n    assert True\n",
    )
    .unwrap();
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        repo,
        &[],
        &[],
        &[],
        &[],
    )
    .expect("store empty python selectors");
    let selector = "tests/test_slow.py::test_slow".to_string();
    write_python_population_manifest_for_args(repo, std::slice::from_ref(&selector), &[])
        .expect("population manifest");
    let manifest = read_python_population_manifest(repo).expect("read manifest");
    let cache_root = python_coverage_cache_root(repo).expect("cache root");
    write_population_durations(
        &cache_root,
        &manifest,
        &[(selector.clone(), Duration::from_secs(5))],
    )
    .expect("durations");
    let max =
        crate::test_runner::python_coverage_index::load_current_python_population_max_duration(
            repo,
            &[],
        );
    assert_eq!(
        max,
        Some(Duration::from_secs(5)),
        "population must be visible"
    );
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &[("*".to_string(), 1.0)],
        timing: false,
        pytest_args: &[],
    });
    match eval {
        RuntimeGateEval::Failed(viols) => {
            assert_eq!(viols.len(), 1);
            assert_eq!(viols[0].selector, selector);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn evaluate_cov_time_gate_does_not_pass_empty_population_when_pytest_args_set() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::write(repo.join("app.py"), "x = 1\n").unwrap();
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        repo,
        &[],
        &[],
        &[],
        &[],
    )
    .expect("store empty python selectors");
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &[("*".to_string(), 1.0)],
        timing: false,
        pytest_args: &["-k".into(), "does_not_match".into()],
    });
    assert_ne!(eval, RuntimeGateEval::Passed);
}

#[test]
fn evaluate_cov_time_gate_disabled_without_limits() {
    let tmp = tempfile::tempdir().unwrap();
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: tmp.path(),
        lang_filter: None,
        include: TimingLangInclude {
            python: true,
            rust: true,
        },
        ignore: &[],
        limits: &[],
        timing: false,
        pytest_args: &[],
    });
    assert_eq!(eval, RuntimeGateEval::Disabled);
}

#[test]
fn evaluate_cov_time_gate_sole_star_from_python_generation() {
    use crate::test_runner::python_coverage_index::generation::{
        population_plan_for_selectors, publish_python_population_generation, PopulationEvidence,
        SelectorEvidence, TimingCacheDisposition,
    };
    use crate::test_runner::python_coverage_index::{
        clear_python_generation_warm_memo, GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION,
    };
    use crate::test_runner::runners::detect_rslip_versions;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "t.py::test_a".to_string();
    let mut plan =
        population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(25)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();

    let limits = vec![("*".to_string(), 1.0)];
    let passed = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &limits,
        timing: true,
        pytest_args: &[],
    });
    assert_eq!(passed, RuntimeGateEval::Passed);

    let tight = vec![("*".to_string(), 0.001)];
    let failed = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &tight,
        timing: false,
        pytest_args: &[],
    });
    assert!(matches!(failed, RuntimeGateEval::Failed(_)));
}

#[test]
fn evaluate_cov_time_gate_multi_prefix_and_incomplete() {
    use crate::test_runner::python_coverage_index::generation::{
        population_plan_for_selectors, publish_python_population_generation, PopulationEvidence,
        SelectorEvidence, TimingCacheDisposition,
    };
    use crate::test_runner::python_coverage_index::{
        clear_python_generation_warm_memo, GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION,
    };
    use crate::test_runner::runners::detect_rslip_versions;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;

    let empty = tempfile::tempdir().unwrap();
    let incomplete = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: empty.path(),
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &[("*".to_string(), 1.0)],
        timing: false,
        pytest_args: &[],
    });
    assert_eq!(incomplete, RuntimeGateEval::Incomplete);

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "tests/fast/a.py::t".to_string();
    let mut plan =
        population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(40)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();

    let limits = vec![("tests/fast".to_string(), 2.0), ("*".to_string(), 0.0)];
    let eval = evaluate_cov_time_gate(CovTimeGateOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        limits: &limits,
        timing: true,
        pytest_args: &[],
    });

    assert!(
        matches!(eval, RuntimeGateEval::Passed | RuntimeGateEval::Failed(_)),
        "unexpected {eval:?}"
    );
}

#[test]
fn python_timings_map_published_population_durations() {
    use crate::test_runner::python_coverage_index::generation::{
        population_plan_for_selectors, publish_python_population_generation, PopulationEvidence,
        SelectorEvidence, TimingCacheDisposition,
    };
    use crate::test_runner::python_coverage_index::{
        clear_python_generation_warm_memo, GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION,
    };
    use crate::test_runner::runners::detect_rslip_versions;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::fs;

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "t.py::test_a".to_string();
    let mut plan =
        population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(9)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();
    let pop = collect_current_unit_test_timings(TimingCollectOpts {
        universe: repo,
        lang_filter: Some(Language::Python),
        include: TimingLangInclude {
            python: true,
            rust: false,
        },
        ignore: &[],
        pytest_args: &[],
    });
    match pop {
        TimingPopulation::Complete(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].selector, selector);
        }
        other => panic!("expected Complete, got {other:?}"),
    }
}
