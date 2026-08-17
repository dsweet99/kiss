//! allow_refresh identity-mismatch refresh path.

use super::*;
#[test]
fn allow_refresh_true_invokes_refresh_on_identity_mismatch() {
    use crate::test_runner::python_coverage_index::generation::{
        PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
        population_plan_for_selectors, publish_python_population_generation,
    };
    use crate::test_runner::python_coverage_index::{
        GenerationReason, clear_python_generation_warm_memo,
    };
    use rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::Duration;

    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::env::set_current_dir(repo).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    fs::write(
        repo.join("test_app.py"),
        b"def test_ok():\n    assert True\n",
    )
    .unwrap();
    let selectors = vec!["test_app.py::test_ok".into()];
    let Ok(mut plan) = population_plan_for_selectors(repo, &selectors, &[]) else {
        return;
    };
    plan.base_identity.input_fingerprint = "stale-fingerprint".into();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: "test_app.py::test_ok".into(),
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

    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    assert!(
        matches!(
            load_or_refresh_snapshot(
                repo,
                required,
                &[],
                1,
                false,
                &kiss::GateConfig::default(),
                &[],
            ),
            Err(1)
        ),
        "allow_refresh false must fail closed on identity mismatch"
    );

    // allow_refresh true must refresh (re-run / republish) so load succeeds.
    let refreshed = load_or_refresh_snapshot(
        repo,
        required,
        &[],
        2,
        true,
        &kiss::GateConfig::default(),
        &[],
    );
    assert!(
        refreshed.is_ok(),
        "allow_refresh true must invoke refresh and repair; got {refreshed:?}"
    );
}

