use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use tempfile::tempdir;

use super::evidence::{PopulationEvidence, SelectorEvidence};
use super::identity::population_plan_for_selectors;
use super::load::try_load_pinned_python_generation_warm;
use super::publish::publish_python_population_generation;
use super::types::{GenerationReason, TimingCacheDisposition};
use crate::test_runner::python_coverage_index::PYTHON_SELECTOR_DISCOVERY_VERSION;
use crate::test_runner::runners::detect_rslip_versions;

fn assert_pointer_records_parent(repo: &Path, expected_parent: &str) {
    let cache =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(repo)
            .unwrap();
    let bytes = std::fs::read(super::paths::pointer_path(&cache)).unwrap();
    let pointer: super::types::PopulationPointer = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(pointer.parent_generation_id, expected_parent);
}

fn assert_published_interned_line_index(repo: &Path) {
    let full = super::load::try_load_pinned_python_generation(repo).unwrap();
    assert_eq!(
        full.line_index.schema_version,
        super::types::InternedLineIndex::default().schema_version
    );
    assert_eq!(
        full.line_index.selectors_for_line("app.py", 1),
        vec!["t.py::test_a"]
    );
}

fn assert_legacy_line_index_interns_without_name_blowup() {
    let long = "tests/fast/pkg/test_module.py::test_with_a_very_long_selector_name";
    let mut legacy: BTreeMap<String, BTreeMap<u32, BTreeSet<String>>> = BTreeMap::new();
    let mut lines = BTreeMap::new();
    for line in 1..=80u32 {
        lines.insert(
            line,
            BTreeSet::from([long.to_string(), format!("{long}_other")]),
        );
    }
    legacy.insert("pkg/mod.py".to_string(), lines);
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    let interned = super::types::decode_line_index_bytes(&legacy_bytes).unwrap();
    assert_eq!(interned.selectors.len(), 2);
    assert_eq!(interned.selectors_for_line("pkg/mod.py", 1).len(), 2);
    let interned_bytes = serde_json::to_vec(&interned).unwrap();
    assert!(
        interned_bytes.len() * 4 < legacy_bytes.len(),
        "interned {} vs legacy {}",
        interned_bytes.len(),
        legacy_bytes.len()
    );
}

fn assert_restamp_rewrites_stale_kissconfig_and_refuses_fingerprint(repo: &Path) {
    let pinned = super::load::try_load_pinned_python_generation(repo).unwrap();
    let mut stale_kc = pinned.plan.clone();
    stale_kc.base_identity.kissconfig_test_digest = "stale-kissconfig".into();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&stale_kc.selectors);
    evidence.coverage = pinned.coverage.clone();
    evidence.selector_coverage = pinned.selector_coverage.clone();
    evidence.timings = pinned.timings.clone();
    evidence.complete = pinned.complete;
    evidence.rebuild_line_index();
    super::publish::publish_python_population_generation(
        repo,
        &stale_kc,
        &evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    let wrote = super::repair::restamp_and_repair_python_population_generation(
        repo,
        &[],
        &[],
        GenerationReason::Complete,
    )
    .unwrap();
    assert!(wrote.is_some());
    let after_kc = try_load_pinned_python_generation_warm(repo).unwrap();
    assert_ne!(
        after_kc.plan.base_identity.kissconfig_test_digest,
        "stale-kissconfig"
    );

    let mut stale_fp = after_kc.plan.clone();
    stale_fp.base_identity.input_fingerprint = "stale-fingerprint".into();
    let mut fp_evidence = PopulationEvidence::from_ordered_selectors(&stale_fp.selectors);
    fp_evidence.coverage = after_kc.coverage.clone();
    fp_evidence.selector_coverage = after_kc.selector_coverage.clone();
    fp_evidence.timings = after_kc.timings.clone();
    fp_evidence.complete = after_kc.complete;
    fp_evidence.rebuild_line_index();
    super::publish::publish_python_population_generation(
        repo,
        &stale_fp,
        &fp_evidence,
        GenerationReason::Complete,
    )
    .unwrap();
    let refused = super::repair::try_restamp_matching_pinned_universe(
        repo,
        &stale_fp.selectors,
        &[],
        &|_, _| true,
        &kiss::GateConfig::default(),
        None,
    )
    .unwrap();
    assert!(
        !refused,
        "unknown run-miss set must rematerialize fingerprint drift"
    );
    assert_eq!(
        try_load_pinned_python_generation_warm(repo)
            .unwrap()
            .plan
            .base_identity
            .input_fingerprint,
        "stale-fingerprint"
    );
    let restamped = super::repair::try_restamp_matching_pinned_universe(
        repo,
        &stale_fp.selectors,
        &[],
        &|_, _| true,
        &kiss::GateConfig::default(),
        Some(&[]),
    )
    .unwrap();
    assert!(
        !restamped,
        "fingerprint drift must not restamp from a source-only selector match"
    );
    assert_eq!(
        try_load_pinned_python_generation_warm(repo)
            .unwrap()
            .plan
            .base_identity
            .input_fingerprint,
        "stale-fingerprint"
    );
}

fn assert_repair_skips_line_index_bytes(repo: &Path) {
    let without_line_index =
        super::load::try_load_pinned_python_generation_without_line_index(repo).unwrap();
    assert!(without_line_index.line_index.files.is_empty());
    assert!(!without_line_index.selector_coverage.is_empty());
    let index = super::load::generation_file_index(&without_line_index);
    assert!(index.contains_key("app.py"));
}

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
    let id =
        publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
            .unwrap();
    assert_pointer_records_parent(repo, "");
    let second =
        publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
            .unwrap();
    assert_pointer_records_parent(repo, &id);
    assert_ne!(second, id);
    let pinned = try_load_pinned_python_generation_warm(repo).unwrap();
    assert_eq!(pinned.generation_id, second);
    assert!(pinned.complete);
    assert_eq!(pinned.coverage.get("app.py"), Some(&BTreeSet::from([1u32])));
    assert_eq!(pinned.timings.len(), 1);
    assert_published_interned_line_index(repo);
    assert_legacy_line_index_interns_without_name_blowup();
    cover_exact_identity_restamp_and_closed_misses(repo, &plan.selectors);
    assert_restamp_rewrites_stale_kissconfig_and_refuses_fingerprint(repo);
}

fn cover_exact_identity_restamp_and_closed_misses(repo: &Path, selectors: &[String]) {
    let _ = super::repair::restamp_complete_pinned_from_cache(
        repo,
        &[],
        &|_, _| true,
        &kiss::GateConfig::default(),
    );
    let matched = super::repair::try_restamp_matching_pinned_universe(
        repo,
        selectors,
        &[],
        &|_, _| true,
        &kiss::GateConfig::default(),
        Some(&[]),
    )
    .unwrap();
    assert!(matched, "exact identity may restamp an unchanged universe");
    assert!(
        !super::repair::try_restamp_matching_pinned_universe(
            repo,
            &["other.py::test_z".into()],
            &[],
            &|_, _| true,
            &kiss::GateConfig::default(),
            None,
        )
        .unwrap()
    );
    let empty = tempdir().unwrap();
    assert!(
        !super::repair::try_restamp_matching_pinned_universe(
            empty.path(),
            selectors,
            &[],
            &|_, _| true,
            &kiss::GateConfig::default(),
            None,
        )
        .unwrap()
    );
    assert!(
        super::repair::repair_python_population_generation(
            repo,
            &[passed_evidence("missing.py::test_z", "app.py", &[1])],
            GenerationReason::SelectiveRepair,
        )
        .is_err()
    );
    let problems =
        super::repair::problem_selectors_from_timings(&[super::types::SelectorTimingRecord {
            selector: "failed.py::t".into(),
            raw_status: "failed".into(),
            effective_status: "failed".into(),
            duration_ns: Some(1),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
            test_definition_digest: String::new(),
        }]);
    assert_eq!(problems, vec!["failed.py::t".to_string()]);
}

#[test]
fn selective_repair_updates_only_changed_selector_coverage() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\ny = 2\n").unwrap();
    std::fs::write(
        repo.join("t.py"),
        b"def test_a(): pass\ndef test_b(): pass\n",
    )
    .unwrap();
    let Ok((_py, _pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::test_a".into(), "t.py::test_b".into()];
    let plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(passed_evidence("t.py::test_a", "app.py", &[1]));
    evidence.absorb_selector(passed_evidence("t.py::test_b", "app.py", &[1, 2]));
    let _ =
        publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
            .unwrap();
    let before = super::load::try_load_pinned_python_generation(repo).unwrap();
    let unchanged = super::repair::repair_python_population_generation(
        repo,
        &[passed_evidence("t.py::test_a", "app.py", &[1])],
        GenerationReason::SelectiveRepair,
    )
    .unwrap();
    assert!(unchanged.is_none(), "matching evidence must not rewrite");
    std::fs::write(
        repo.join("t.py"),
        b"def test_a(): pass  # changed\ndef test_b(): pass\n",
    )
    .unwrap();
    let digest_repaired = super::repair::repair_python_population_generation(
        repo,
        &[passed_evidence("t.py::test_a", "app.py", &[1])],
        GenerationReason::SelectiveRepair,
    )
    .unwrap();
    assert!(
        digest_repaired.is_some(),
        "changed test definition must rewrite matching evidence"
    );
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
    assert_eq!(
        after.line_index.selectors_for_line("app.py", 1),
        vec!["t.py::test_b"]
    );
    assert_eq!(
        after.line_index.selectors_for_line("app.py", 2),
        vec!["t.py::test_a", "t.py::test_b"]
    );
    assert_repair_skips_line_index_bytes(repo);
}
