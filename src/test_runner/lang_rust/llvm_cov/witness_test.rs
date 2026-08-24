use std::collections::BTreeMap;
use std::path::PathBuf;

use rust_llvm_cov_runner::{
    CoverageOutputMode, RustCoverageBatchIdentity, RustCoverageBatchRequest,
};

use super::{align_statuses, full_publication_selectors, merged_statuses};
use crate::test_runner::execution_witness::{ExecutionWitness, WitnessScope, WitnessStatus};
use crate::test_runner::runners::SelectorExecutionSummary;

fn publish_test_identity() -> RustCoverageBatchIdentity {
    RustCoverageBatchIdentity {
        input_digest: "publish-input".into(),
        generation_fingerprint: "publish-gen".into(),
        selection_context_fingerprint: "publish-sel".into(),
        ordinary_source_digests: Default::default(),
    }
}

fn sample_req(
    logical: &[&str],
    mode: CoverageOutputMode,
    population: Option<Vec<String>>,
) -> RustCoverageBatchRequest {
    RustCoverageBatchRequest {
        cwd: PathBuf::from("/repo"),
        source_root: PathBuf::from("/repo"),
        cargo: PathBuf::from("cargo"),
        cache_root: PathBuf::from("/tmp/nonexistent-kiss-cache"),
        logical_selectors: logical.iter().map(|s| (*s).to_string()).collect(),
        cargo_args: vec!["--workspace".into()],
        test_args: vec![],
        env: BTreeMap::new(),
        force_rerun: false,
        jobs: 1,
        generated_config: PathBuf::from("/tmp/nextest.toml"),
        population_publication_selectors: population,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: "0".into(),
        host_platform: "x86_64-unknown-linux-gnu".into(),
        coverage_output_mode: mode,
        selector_timeout_millis: BTreeMap::new(),
    }
}

fn full_witness(selectors: &[&str]) -> ExecutionWitness {
    ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "rs:publish-input:publish-gen:publish-sel".into(),
        selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        statuses: vec![WitnessStatus::Passed; selectors.len()],
        durations_ns: vec![Some(0); selectors.len()],
        covered_lines: Default::default(),
        complete: true,
        generation_id: "rust-wit-test".into(),
    }
}

fn check_aggregate_req(logical: &[&str]) -> RustCoverageBatchRequest {
    sample_req(
        logical,
        CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        },
        None,
    )
}

#[test]
fn check_aggregate_repair_merges_into_existing_full_universe() {
    let existing = full_witness(&["a", "b", "c", "d"]);
    let req = check_aggregate_req(&["b", "c"]);
    let by_logical = BTreeMap::from([
        ("a".into(), WitnessStatus::Passed),
        ("b".into(), WitnessStatus::Passed),
        ("c".into(), WitnessStatus::Passed),
        ("d".into(), WitnessStatus::Passed),
    ]);
    let selectors = full_publication_selectors(
        &req,
        std::path::Path::new("/tmp"),
        &publish_test_identity(),
        Some(&existing),
        &by_logical,
    )
    .expect("should publish Full");
    assert_eq!(selectors, vec!["a", "b", "c", "d"]);
}

#[test]
fn check_aggregate_subset_without_full_base_does_not_claim_full() {
    let req = check_aggregate_req(&["b", "c"]);
    let by_logical = BTreeMap::from([
        ("b".into(), WitnessStatus::Passed),
        ("c".into(), WitnessStatus::Passed),
    ]);
    assert!(
        full_publication_selectors(
            &req,
            std::path::Path::new("/tmp"),
            &publish_test_identity(),
            None,
            &by_logical,
        )
        .is_none()
    );
}

#[test]
fn population_publication_never_shrinks_existing_full() {
    let existing = full_witness(&["a", "b", "c"]);
    let req = sample_req(
        &["a"],
        CoverageOutputMode::SelectorEntries,
        Some(vec!["a".into()]),
    );
    let by_logical = BTreeMap::from([
        ("a".into(), WitnessStatus::Passed),
        ("b".into(), WitnessStatus::Passed),
        ("c".into(), WitnessStatus::Passed),
    ]);
    let selectors = full_publication_selectors(
        &req,
        std::path::Path::new("/tmp"),
        &publish_test_identity(),
        Some(&existing),
        &by_logical,
    )
    .expect("merge");
    assert_eq!(selectors, vec!["a", "b", "c"]);
}

#[test]
fn align_statuses_requires_complete_universe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() {}\n    #[test]\n    fn b() {}\n}\n",
    )
    .unwrap();
    let by = BTreeMap::from([("tests::a".into(), WitnessStatus::Passed)]);
    let summary = SelectorExecutionSummary {
        selector_durations_ns: BTreeMap::from([("tests::a".into(), 42)]),
        ..Default::default()
    };
    assert!(
        align_statuses(
            tmp.path(),
            &["tests::a".into(), "tests::b".into()],
            &by,
            &summary,
            None
        )
        .unwrap()
        .is_none()
    );
    let aligned = align_statuses(tmp.path(), &["tests::a".into()], &by, &summary, None)
        .unwrap()
        .unwrap();
    assert_eq!(aligned.statuses, vec![WitnessStatus::Passed]);
    assert_eq!(aligned.durations_ns, vec![Some(42)]);
}

#[test]
fn align_statuses_preserves_missing_duration_as_none() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() {}\n}\n",
    )
    .unwrap();
    let by = BTreeMap::from([("tests::a".into(), WitnessStatus::Passed)]);
    let summary = SelectorExecutionSummary::default();
    let aligned = align_statuses(tmp.path(), &["tests::a".into()], &by, &summary, None)
        .unwrap()
        .unwrap();
    assert_eq!(aligned.statuses, vec![WitnessStatus::Passed]);
    assert_eq!(aligned.durations_ns, vec![None]);
}

#[test]
fn merged_statuses_overlays_repair_on_existing_full() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() {}\n    #[test]\n    fn b() {}\n}\n",
    )
    .unwrap();
    let existing = full_witness(&["tests::a", "tests::b"]);
    let req = check_aggregate_req(&["tests::b"]);
    let summary = SelectorExecutionSummary {
        total: 1,
        failed: 0,
        ..Default::default()
    };
    let merged = merged_statuses(tmp.path(), &req, &summary, Some(&existing)).unwrap();
    assert_eq!(merged.get("tests::a"), Some(&WitnessStatus::Passed));
    assert_eq!(merged.get("tests::b"), Some(&WitnessStatus::Passed));
}

#[test]
fn publish_refuses_to_shrink_full_via_store_guard() {
    use crate::test_runner::execution_witness::{
        PublishRustWitness, publish_rust_execution_witness, try_load_rust_execution_witness,
    };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let identity = publish_test_identity();
    let full = vec!["a".into(), "b".into(), "c".into()];
    let shrink = vec!["a".into()];
    let empty_cov = Default::default();
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &full,
        statuses: &[WitnessStatus::Passed; 3],
        durations_ns: &[Some(0); 3],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    let before = try_load_rust_execution_witness(tmp.path()).unwrap();
    let shrunk = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &shrink,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(0)],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    assert_eq!(shrunk, before.generation_id);
    let after = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(after.selectors, full);
}
