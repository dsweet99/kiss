//! Generation publish/load focused tests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use tempfile::tempdir;

use super::evidence::{PopulationEvidence, SelectorEvidence};
use super::identity::population_plan_for_selectors;
use super::load::try_load_pinned_python_generation_warm;
use super::publish::publish_python_population_generation;
use super::types::{GenerationReason, TimingCacheDisposition};
use crate::test_runner::python_coverage_index::PYTHON_SELECTOR_DISCOVERY_VERSION;
use crate::test_runner::runners::detect_rslip_versions;

fn passed_evidence(selector: &str, file: &str, lines: &[u32]) -> SelectorEvidence {
    SelectorEvidence {
        selector: selector.to_string(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(3)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([(file.to_string(), lines.iter().copied().collect())]),
    }
}

#[test]
fn publish_then_warm_load_reads_coverage_and_timings() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();

    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let mut plan = population_plan_for_selectors(repo, &["t.py::test_a".into()], &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(passed_evidence("t.py::test_a", "app.py", &[1]));
    let id = publish_python_population_generation(
        repo,
        &plan,
        &evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    let pinned = try_load_pinned_python_generation_warm(repo).unwrap();
    assert_eq!(pinned.generation_id, id);
    assert!(pinned.complete);
    assert_eq!(
        pinned.coverage.get("app.py"),
        Some(&BTreeSet::from([1u32]))
    );
    assert_eq!(pinned.timings.len(), 1);
    let _ = Path::new(".");
}

#[test]
fn selective_repair_updates_only_changed_selector_coverage() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\ny = 2\n").unwrap();
    let Ok((_py, _pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::test_a".into(), "t.py::test_b".into()];
    let plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(passed_evidence("t.py::test_a", "app.py", &[1]));
    evidence.absorb_selector(passed_evidence("t.py::test_b", "app.py", &[1, 2]));
    let _ = publish_python_population_generation(
        repo,
        &plan,
        &evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    let before = super::load::try_load_pinned_python_generation(repo).unwrap();
    let unchanged = super::repair::repair_python_population_generation(
        repo,
        &[passed_evidence("t.py::test_a", "app.py", &[1])],
        GenerationReason::SelectiveRepair,
    )
    .unwrap();
    assert!(unchanged.is_none(), "matching evidence must not rewrite");
    let changed = super::repair::repair_python_population_generation(
        repo,
        &[passed_evidence("t.py::test_a", "app.py", &[2])],
        GenerationReason::SelectiveRepair,
    )
    .unwrap();
    assert!(changed.is_some());
    let after = super::load::try_load_pinned_python_generation(repo).unwrap();
    assert_ne!(before.generation_id, after.generation_id);
    assert_eq!(
        after.coverage.get("app.py"),
        Some(&BTreeSet::from([1u32, 2]))
    );
    assert_eq!(
        after.selector_coverage.get("t.py::test_a"),
        Some(&BTreeMap::from([(
            "app.py".to_string(),
            BTreeSet::from([2u32])
        )]))
    );
}
