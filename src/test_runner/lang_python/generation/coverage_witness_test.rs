use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use tempfile::tempdir;

use super::current::{
    current_complete_generation_matches, current_generation_matches_plan,
    current_identity_fingerprint, generation_matches, load_generation_or_stale,
};
use super::evidence::{PopulationEvidence, SelectorEvidence};
use super::identity::population_plan_for_selectors;
use super::load::{try_load_pinned_python_generation, try_load_pinned_python_generation_warm};
use super::memo::{
    clear_python_generation_warm_memo, try_load_pinned_python_generation_warm_memoized,
};
use super::publish::publish_python_population_generation;
use super::repair::{
    problem_selectors_from_timings, repair_python_population_generation,
    restamp_complete_pinned_from_cache,
};
use super::types::{GenerationReason, TimingCacheDisposition};
use crate::test_runner::python_coverage_index::PYTHON_SELECTOR_DISCOVERY_VERSION;
use crate::test_runner::runners::detect_rslip_versions;

fn publish_one(repo: &Path, selector: &str) -> String {
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return String::new();
    };
    let mut plan = population_plan_for_selectors(repo, &[selector.to_string()], &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    let mut lines = BTreeSet::new();
    lines.insert(1u32);
    let mut cov = BTreeMap::new();
    cov.insert("app.py".to_string(), lines);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.to_string(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(3)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: cov,
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap()
}

#[test]
fn current_helpers_and_identity_fingerprint() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let selector = "t.py::test_a";
    let id = publish_one(repo, selector);
    assert!(!id.is_empty());
    assert!(current_complete_generation_matches(
        repo,
        &[selector.to_string()],
        &[]
    ));
    let pinned = current_generation_matches_plan(repo, &[selector.to_string()], &[]).unwrap();
    assert!(generation_matches(
        &pinned,
        repo,
        &[selector.to_string()],
        &[]
    ));
    assert!(!generation_matches(
        &pinned,
        repo,
        &["other.py::t".into()],
        &[]
    ));
    assert!(load_generation_or_stale(repo).is_ok());
    assert!(current_identity_fingerprint(repo, &[]).is_some());
    assert!(
        restamp_complete_pinned_from_cache(repo, &[], &|_, _| true, &kiss::GateConfig::default(),)
            .unwrap()
    );
}

#[test]
fn problem_selectors_and_out_of_universe_repair() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let id = publish_one(repo, "t.py::test_a");
    assert!(!id.is_empty());
    let pinned = try_load_pinned_python_generation(repo).unwrap();
    let problems = problem_selectors_from_timings(&pinned.timings);
    assert!(problems.is_empty());
    let err = repair_python_population_generation(
        repo,
        &[SelectorEvidence {
            selector: "missing.py::t".into(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: None,
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: None,
            coverage: BTreeMap::new(),
        }],
        GenerationReason::SelectiveRepair,
    )
    .unwrap_err();
    assert!(err.contains("outside the current Python population"));
}

#[test]
fn cold_selective_repair_is_noop_without_population() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    let repaired = repair_python_population_generation(
        repo,
        &[SelectorEvidence {
            selector: "t.py::test_a".into(),
            raw_status: TestStatus::Passed,
            effective_status: TestStatus::Passed,
            duration: None,
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: None,
            coverage: BTreeMap::new(),
        }],
        GenerationReason::IncompleteRepair,
    )
    .expect("cold selective repair must not fail");
    assert_eq!(repaired, None);
}

#[test]
fn corrupt_pointer_does_not_prune_finalized_generations() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let id = publish_one(repo, "t.py::test_a");
    assert!(!id.is_empty());
    let cache =
        crate::test_runner::python_coverage_index::python_coverage_cache_root(repo).unwrap();
    let gen_dir = cache.join("generations").join(&id);
    assert!(gen_dir.is_dir());
    fs::write(cache.join("population.json"), b"{not-json").unwrap();
    clear_python_generation_warm_memo();
    assert!(try_load_pinned_python_generation_warm(repo).is_err());
    assert!(
        gen_dir.is_dir(),
        "corrupt pointer must not delete finalized generation"
    );
}

#[test]
fn warm_memo_returns_cached_generation_without_rewrite() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let id = publish_one(repo, "t.py::test_a");
    assert!(!id.is_empty());
    clear_python_generation_warm_memo();
    let a = try_load_pinned_python_generation_warm_memoized(repo).unwrap();
    let b = try_load_pinned_python_generation_warm_memoized(repo).unwrap();
    assert_eq!(a.generation_id, b.generation_id);
}

#[test]
fn warm_memo_observes_external_pointer_advance() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let first = publish_one(repo, "t.py::test_a");
    let cache =
        crate::test_runner::python_coverage_index::python_coverage_cache_root(repo).unwrap();
    let pointer = cache.join("population.json");
    let first_pointer = fs::read(&pointer).unwrap();
    let second = publish_one(repo, "t.py::test_b");
    let second_pointer = fs::read(&pointer).unwrap();

    fs::write(&pointer, first_pointer).unwrap();
    clear_python_generation_warm_memo();
    assert_eq!(
        try_load_pinned_python_generation_warm_memoized(repo)
            .unwrap()
            .generation_id,
        first
    );
    fs::write(&pointer, second_pointer).unwrap();
    assert_eq!(
        try_load_pinned_python_generation_warm_memoized(repo)
            .unwrap()
            .generation_id,
        second
    );
}

#[test]
fn incomplete_timings_list_problem_selectors() {
    use super::types::SelectorTimingRecord;
    let timings = vec![
        SelectorTimingRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            effective_status: "passed".into(),
            duration_ns: Some(1),
            cache_disposition: TimingCacheDisposition::Hit,
            reason: None,
            test_definition_digest: String::new(),
        },
        SelectorTimingRecord {
            selector: "b".into(),
            raw_status: "failed".into(),
            effective_status: "failed".into(),
            duration_ns: None,
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: Some("assert".into()),
            test_definition_digest: String::new(),
        },
        SelectorTimingRecord {
            selector: "c".into(),
            raw_status: "unresolved".into(),
            effective_status: "unresolved".into(),
            duration_ns: None,
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: Some("missing".into()),
            test_definition_digest: String::new(),
        },
    ];
    assert_eq!(
        problem_selectors_from_timings(&timings),
        vec!["b".to_string(), "c".to_string()]
    );
}
