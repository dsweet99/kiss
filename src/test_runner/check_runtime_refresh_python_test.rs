//! Unit coverage for Python incomplete-generation refresh helpers.

use super::check_runtime_refresh_python::{
    ensure_python_runtime_coverage, finalize_incomplete_repair_load,
    incomplete_repair_became_test_failure,
};
use crate::test_runner::check_line_coverage::{
    RuntimeCoverageLoadError, load_python_runtime_coverage,
};
use crate::test_runner::python_coverage_index::generation::{
    PopulationEvidence, SelectorEvidence, TimingCacheDisposition, population_plan_for_selectors,
    publish_python_population_generation,
};
use crate::test_runner::python_coverage_index::{
    GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION, clear_python_generation_warm_memo,
};
use crate::test_runner::runners::{SelectorExecutionSummary, detect_rslip_versions};
use rpytest_runner::TestStatus;
use std::collections::BTreeMap;
use std::time::Duration;

#[test]
fn ensure_python_short_circuits_when_complete_generation_present() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "t.py::test_a".to_string();
    let mut plan = population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();
    assert!(load_python_runtime_coverage(repo, &[], &kiss::GateConfig::default()).is_ok());
    ensure_python_runtime_coverage(repo, &[], 1, &[], &kiss::GateConfig::default()).expect("already complete");
}

#[test]
fn ensure_python_attempts_incomplete_repair_for_problem_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::ok".into(), "t.py::bad".into()];
    let mut plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    plan.base_identity.python_version = py.clone();
    plan.base_identity.pytest_version = pt.clone();
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::ok".into(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::bad".into(),
        raw_status: TestStatus::Failed,
        effective_status: TestStatus::Failed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: Some("boom".into()),
        coverage: BTreeMap::new(),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();
    let err = load_python_runtime_coverage(repo, &[], &kiss::GateConfig::default()).expect_err("incomplete");
    assert_eq!(err.problem_selectors, vec!["t.py::bad".to_string()]);
    // Repair attempts to rerun only problem selectors; without a real pytest node it fails closed.
    let _ = ensure_python_runtime_coverage(repo, &[], 1, &[], &kiss::GateConfig::default());
}

#[test]
fn incomplete_repair_classifier_and_finalize_arms() {
    let err = RuntimeCoverageLoadError {
        language: "Python",
        reason: "incomplete population".into(),
        problem_selectors: vec!["t.py::bad".into()],
    };
    assert!(incomplete_repair_became_test_failure(&err, 1));
    assert!(!incomplete_repair_became_test_failure(&err, 0));
    assert!(!incomplete_repair_became_test_failure(
        &RuntimeCoverageLoadError {
            language: "Python",
            reason: "missing".into(),
            problem_selectors: Vec::new(),
        },
        1
    ));

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::ok".into(), "t.py::bad".into()];
    let mut plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::ok".into(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::bad".into(),
        raw_status: TestStatus::Failed,
        effective_status: TestStatus::Failed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: Some("boom".into()),
        coverage: BTreeMap::new(),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();

    let failed_summary = SelectorExecutionSummary {
        total: 1,
        failed: 1,
        exit_code: 1,
        ..SelectorExecutionSummary::default()
    };
    let err = finalize_incomplete_repair_load(
        repo,
        &failed_summary,
        &[],
        &kiss::GateConfig::default(),
    )
    .expect_err("still incomplete");
    assert!(matches!(
        err,
        super::CoverageRefreshError::TestExecution { exit_code: 1, .. }
    ));
    let ok_summary = SelectorExecutionSummary::default();
    let err = finalize_incomplete_repair_load(
        repo,
        &ok_summary,
        &[],
        &kiss::GateConfig::default(),
    )
    .expect_err("incomplete + exit0");
    assert!(matches!(
        err,
        super::CoverageRefreshError::PostRefreshValidation { .. }
    ));
}

#[test]
fn ensure_python_falls_through_to_full_refresh_without_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    // No population/generation: cold path runs discovery + refresh (fails closed without tests).
    let _ = ensure_python_runtime_coverage(repo, &[], 1, &[], &kiss::GateConfig::default());
}
