use super::{
    rust_plan_selectors, rust_population_current_for_all_selectors,
    rust_witness_accepts_full_universe,
};
use crate::test_runner::execution_witness::{
    PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
    rust_identity_digest_from_batch,
};
use kiss::GateConfig;
use std::collections::{BTreeMap, BTreeSet};

fn identity() -> kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
    kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "i".into(),
        generation_fingerprint: "g".into(),
        selection_context_fingerprint: "s".into(),
        ordinary_source_digests: Default::default(),
    }
}

fn demo_lib(tmp: &tempfile::TempDir) {
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

#[test]
fn rust_witness_accept_helpers_fail_closed_without_witness() {
    let tmp = tempfile::tempdir().unwrap();
    let id = identity();
    let selectors = vec!["a".into()];
    let gate = GateConfig::default();
    assert!(!rust_witness_accepts_full_universe(
        tmp.path(),
        &selectors,
        &id,
        &gate
    ));
    assert!(!rust_population_current_for_all_selectors(
        tmp.path(),
        &selectors,
        &gate
    ));
}

#[test]
fn rust_population_current_unreadable_population_without_witness_is_not_current() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("population.json"), b"{}").unwrap();
    assert!(!rust_population_current_for_all_selectors(
        tmp.path(),
        &["a".into()],
        &GateConfig::default()
    ));
}

#[test]
fn rust_population_current_when_published_population_matches() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
        tmp.path(),
        &["a".into()],
        &[],
    )
    .unwrap();
    assert!(rust_population_current_for_all_selectors(
        tmp.path(),
        &["a".into()],
        &GateConfig::default()
    ));
}

#[test]
fn rust_population_current_uses_matching_witness_when_population_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    let id = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &[],
    )
    .unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("population.json"), b"{not-json").unwrap();
    let selectors = vec!["a".into(), "b".into()];
    let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &id,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
        durations_ns: &[Some(1), Some(1)],
        covered_lines: &covered,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    assert!(rust_population_current_for_all_selectors(
        tmp.path(),
        &selectors,
        &GateConfig::default()
    ));
    let other = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "other".into(),
        generation_fingerprint: "g".into(),
        selection_context_fingerprint: "s".into(),
        ordinary_source_digests: Default::default(),
    };
    assert!(!rust_witness_accepts_full_universe(
        tmp.path(),
        &selectors,
        &other,
        &GateConfig::default()
    ));
}

#[test]
fn rust_witness_accepts_complete_full_universe() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    let id = identity();
    let selectors = vec!["a".into(), "b".into()];
    let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &id,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
        durations_ns: &[Some(1), Some(1)],
        covered_lines: &covered,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    assert!(!rust_identity_digest_from_batch(&id).is_empty());
    let gate = GateConfig::default();
    assert!(rust_witness_accepts_full_universe(
        tmp.path(),
        &selectors,
        &id,
        &gate
    ));

    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &id,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Unresolved],
        durations_ns: &[Some(1), Some(1)],
        covered_lines: &covered,
        complete: false,
        jobs: 1,
    })
    .unwrap();
    assert!(
        !rust_witness_accepts_full_universe(tmp.path(), &selectors, &id, &gate),
        "a newer incomplete witness must supersede an older passing generation"
    );

    let json_only = tempfile::tempdir().unwrap();
    demo_lib(&json_only);
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: json_only.path(),
        identity: &id,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Unresolved],
        durations_ns: &[Some(1), Some(1)],
        covered_lines: &covered,
        complete: false,
        jobs: 1,
    })
    .unwrap();
    assert!(!rust_witness_accepts_full_universe(
        json_only.path(),
        &selectors,
        &id,
        &gate
    ));
}

#[test]
fn rust_plan_selectors_requires_population_when_cache_is_subset() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
        tmp.path(),
        &["a".into()],
        &[],
    )
    .unwrap();
    let plan = rust_plan_selectors(
        tmp.path(),
        vec!["a".into(), "b".into()],
        &GateConfig::default(),
    );
    assert!(plan.population_required);
    assert_eq!(plan.planned, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(plan.classification.mandatory_misses, vec!["b".to_string()]);
    assert_eq!(plan.classification.intersection, vec!["a".to_string()]);
    assert!(plan.classification.deleted_candidates.is_empty());
}

#[test]
fn rust_plan_selectors_keeps_discovered_when_witness_is_subset() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    let id = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &[],
    )
    .unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("population.json"), b"{not-json").unwrap();
    let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &id,
        scope: WitnessScope::Full,
        selectors: &["a".into(), "b".into()],
        statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
        durations_ns: &[Some(1), Some(1)],
        covered_lines: &covered,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    let plan = rust_plan_selectors(
        tmp.path(),
        vec!["a".into(), "b".into(), "c".into()],
        &GateConfig::default(),
    );
    assert!(!plan.population_required);
    assert_eq!(
        plan.planned,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert_eq!(plan.classification.mandatory_misses, vec!["c".to_string()]);
}

#[test]
fn rust_plan_selectors_drops_deleted_cached_selector() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
        tmp.path(),
        &["a".into(), "z".into()],
        &[],
    )
    .unwrap();
    let plan = rust_plan_selectors(tmp.path(), vec!["a".into()], &GateConfig::default());
    assert_eq!(plan.planned, vec!["a".to_string()]);
    assert_eq!(
        plan.classification.deleted_candidates,
        vec!["z".to_string()]
    );
    assert!(!plan.planned.iter().any(|selector| selector == "z"));
}

#[test]
fn rust_plan_selectors_ignores_stale_population_identity_for_membership() {
    let tmp = tempfile::tempdir().unwrap();
    demo_lib(&tmp);
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("population.json"),
        br#"{"selectors":["stale_only"]}"#,
    )
    .unwrap();
    let plan = rust_plan_selectors(
        tmp.path(),
        vec!["a".into(), "b".into()],
        &GateConfig::default(),
    );
    assert_eq!(plan.planned, vec!["a".to_string(), "b".to_string()]);
    assert!(plan.population_required);
    assert!(!plan.planned.iter().any(|selector| selector == "stale_only"));
}
