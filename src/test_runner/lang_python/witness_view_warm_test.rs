
use super::witness_view::try_warm_python_cached_summary;
use crate::test_runner::python_coverage_index::generation::{
    PopulationEvidence, SelectorEvidence, TimingCacheDisposition, population_plan_for_selectors,
    publish_python_population_generation,
};
use crate::test_runner::python_coverage_index::{
    GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION, clear_python_generation_warm_memo,
};
use crate::test_runner::runners::detect_rslip_versions;
use rpytest_runner::TestStatus;
use std::collections::BTreeMap;
use std::time::Duration;

#[test]
fn try_warm_python_accepts_complete_matching_plan() {
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
    let warm = try_warm_python_cached_summary(repo, &[selector], &[]);
    assert!(warm.is_some());
    assert!(try_warm_python_cached_summary(repo, &["missing".into()], &[]).is_none());
}
