
use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use rpytest_runner::TestStatus;

use crate::test_runner::lang_python::try_warm_python_cached_summary;
use crate::test_runner::python_coverage_index::generation::{
    PopulationEvidence, SelectorEvidence, TimingCacheDisposition, identity_matches_current,
    population_plan_for_selectors, publish_python_population_generation,
};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;
use crate::test_runner::python_coverage_index::{
    GenerationReason, clear_python_generation_warm_memo,
};
use crate::test_runner::runners::detect_rslip_versions;

#[test]
fn generation_identity_mismatch_does_not_fall_through_to_v1_generic() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((_py, _pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::ok".into()];
    let mut plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    plan.base_identity.input_fingerprint = "stale-fingerprint".into();
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
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();
    let err = load_python_runtime_coverage(repo, &[], &kiss::GateConfig::default())
        .expect_err("mismatched identity");
    assert!(
        err.reason.contains("generation identity mismatch"),
        "got: {}",
        err.reason
    );
    assert!(
        !err.reason.contains("missing or stale/incompatible population"),
        "must not fall through to v1 generic; got: {}",
        err.reason
    );
}

#[test]
fn drifted_fingerprint_rejected_by_ensure_and_cov_even_with_warm_seal() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((_py, _pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::ok".into()];
    let mut plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    plan.base_identity.input_fingerprint = "stale-fingerprint".into();
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
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    let cache_root = python_coverage_cache_root(repo).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    fs::write(
        cache_root.join("warm_hit_seal.json"),
        b"{\"schema_version\":\"rslip-warm-hit-v3\"}\n",
    )
    .unwrap();
    clear_python_generation_warm_memo();

    assert!(!identity_matches_current(repo, &plan.base_identity, &[]));
    assert!(try_warm_python_cached_summary(repo, &selectors, &[]).is_none());
    let err = load_python_runtime_coverage(repo, &[], &kiss::GateConfig::default())
        .expect_err("cov must reject drifted fingerprint");
    assert!(
        err.reason.contains("generation identity mismatch"),
        "got: {}",
        err.reason
    );
}
