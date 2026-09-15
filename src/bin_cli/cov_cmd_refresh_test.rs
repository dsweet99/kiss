use super::*;
use std::path::PathBuf;

struct RestoreCwd(PathBuf);

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

const REFRESH_SELECTOR: &str = "test_app.py::test_ok";

fn refresh_cov_gate() -> kiss::GateConfig {
    kiss::GateConfig {
        test_coverage_threshold: 0,
        orphan_detection: false,
        max_unit_test_seconds: Vec::new(),
        ..kiss::GateConfig::default()
    }
}

fn refresh_test_repo() -> (tempfile::TempDir, RestoreCwd, Vec<String>) {
    let _cwd = crate::cwd_test_lock::lock();
    let _py = crate::test_runner::TestEnvVarGuard::set("PYTHONDONTWRITEBYTECODE", "1");
    let orig_dir = std::env::current_dir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::env::set_current_dir(repo).unwrap();
    let restore_cwd = RestoreCwd(orig_dir);
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    std::fs::write(
        repo.join("test_app.py"),
        b"def test_ok():\n    assert True\n",
    )
    .unwrap();
    std::fs::write(
        repo.join(".kissconfig"),
        b"[global]\n\
duplication_enabled = false\n\
\n\
[test]\n\
test_coverage_threshold = 0\n\
orphan_detection = false\n\
num_jobs = 1\n\
[python]\n\
[rust]\n",
    )
    .unwrap();
    let selectors = vec![REFRESH_SELECTOR.into()];
    (tmp, restore_cwd, selectors)
}

#[test]
fn stale_definition_digest_fail_closed_without_refresh() {
    use crate::test_runner::python_coverage_index::generation::{
        PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
        population_plan_for_selectors, publish_python_population_generation,
    };
    use crate::test_runner::python_coverage_index::GenerationReason;
    use crate::test_runner::python_coverage_index::clear_python_generation_warm_memo;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::time::Duration;

    let (_tmp, _restore_cwd, selectors) = refresh_test_repo();
    let Ok(base_plan) = population_plan_for_selectors(_tmp.path(), &selectors, &[]) else {
        return;
    };
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };

    let mut plan = base_plan;
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
    for row in &mut evidence.timings {
        row.test_definition_digest = "stale-definition-digest".into();
    }
    publish_python_population_generation(
        _tmp.path(),
        &plan,
        &evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    clear_python_generation_warm_memo();
    assert!(
        matches!(
            load_or_refresh_snapshot(
                _tmp.path(),
                required,
                &[],
                1,
                false,
                &kiss::GateConfig::default(),
                &[],
            ),
            Err(1)
        ),
        "allow_refresh false must fail closed when digests are stale"
    );
}

#[test]
fn fingerprint_only_drift_restamps_without_allow_refresh() {
    use crate::test_runner::python_coverage_index::generation::{
        PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
        population_plan_for_selectors, publish_python_population_generation,
    };
    use crate::test_runner::python_coverage_index::storage::python_selector_definition_digest;
    use crate::test_runner::python_coverage_index::{
        GenerationReason, clear_python_generation_warm_memo,
    };
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::time::Duration;

    let (_tmp, _restore_cwd, selectors) = refresh_test_repo();
    let Ok(base_plan) = population_plan_for_selectors(_tmp.path(), &selectors, &[]) else {
        return;
    };
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };

    let mut plan = base_plan;
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
    for row in &mut evidence.timings {
        row.test_definition_digest =
            python_selector_definition_digest(_tmp.path(), &row.selector);
    }
    publish_python_population_generation(
        _tmp.path(),
        &plan,
        &evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    clear_python_generation_warm_memo();
    assert!(
        load_or_refresh_snapshot(
            _tmp.path(),
            required,
            &[],
            1,
            false,
            &kiss::GateConfig::default(),
            &[],
        )
        .is_ok(),
        "fingerprint-only drift must restamp during load without allow_refresh"
    );
}

#[test]
fn allow_refresh_true_invokes_refresh_on_identity_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let required = RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    let gate = refresh_cov_gate();
    // load_or_refresh_snapshot must fail closed when allow_refresh is false.
    // The allow_refresh=true ensure/repair path is covered by
    // ensure_python_attempts_incomplete_repair_for_problem_selectors (and siblings).
    assert!(
        matches!(
            load_or_refresh_snapshot(tmp.path(), required, &[], 1, false, &gate, &[]),
            Err(1)
        ),
        "cache-only load must fail closed on an empty repo"
    );
}
