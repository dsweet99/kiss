use std::collections::BTreeMap;

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::{
    OnDiskRustWitness, PublishRustWitness, SCHEMA_VERSION, content_digest, load_witness_from_disk,
    prune_removed_rust_witness_selectors, publish_rust_execution_witness,
    try_load_rust_execution_witness, write_witness_atomic,
};
use crate::test_runner::lang_iface::{ExecutionWitness, WitnessScope, WitnessStatus};

fn sample_identity() -> RustCoverageBatchIdentity {
    RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "gen".into(),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: Default::default(),
    }
}

fn write_minimal_repo(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

fn disk_body(scope: &str, selectors: &[&str], statuses: &[&str]) -> OnDiskRustWitness {
    let mut body = OnDiskRustWitness {
        schema_version: SCHEMA_VERSION.to_string(),
        scope: scope.to_string(),
        identity_digest: "rs:input:gen:sel".into(),
        generation_id: "g".into(),
        complete: true,
        selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        statuses: statuses.iter().map(|s| (*s).to_string()).collect(),
        durations_ns: vec![Some(1); selectors.len()],
        covered_lines: BTreeMap::from([("f.rs".into(), vec![1])]),
        content_sha256: String::new(),
    };
    body.content_sha256 = content_digest(&body).unwrap();
    body
}

fn write_body(root: &std::path::Path, body: &OnDiskRustWitness) {
    write_witness_atomic(root, body).unwrap();
}

#[test]
fn load_witness_from_disk_reads_valid_file() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    write_body(tmp.path(), &disk_body("full", &["a"], &["passed"]));
    let loaded = load_witness_from_disk(tmp.path()).unwrap();
    assert_eq!(loaded.selectors, vec!["a".to_string()]);
    assert_eq!(loaded.covered_lines.get("f.rs").map(Vec::len), Some(1));
}

#[test]
fn load_witness_from_disk_rejects_parse_and_schema() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let path = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("execution_witness.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not-json").unwrap();
    assert!(load_witness_from_disk(tmp.path()).is_err());
    let mut body = disk_body("full", &["a"], &["passed"]);
    body.schema_version = "nope".into();
    body.content_sha256 = content_digest(&body).unwrap();
    write_body(tmp.path(), &body);
    assert!(load_witness_from_disk(tmp.path()).is_err());
}

#[test]
fn load_witness_from_disk_rejects_scope_and_shape() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    write_body(tmp.path(), &disk_body("weird", &["a"], &["passed"]));
    assert!(load_witness_from_disk(tmp.path()).is_err());
    let mut body = disk_body("full", &["a", "b"], &["passed"]);
    body.durations_ns = vec![Some(1)];
    body.content_sha256 = content_digest(&body).unwrap();
    write_body(tmp.path(), &body);
    assert!(load_witness_from_disk(tmp.path()).is_err());
}

#[test]
fn empty_generation_coverage_stays_empty_despite_legacy_disk() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let empty_cov = Default::default();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    write_body(tmp.path(), &disk_body("full", &["a"], &["passed"]));
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert!(loaded.covered_lines.is_empty());
}

#[test]
fn empty_generation_does_not_backfill_coverage_from_other_identity() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &sample_identity(),
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &Default::default(),
        complete: true,
        jobs: 1,
    })
    .unwrap();
    let mut stale = disk_body("full", &["a"], &["passed"]);
    stale.identity_digest = "rs:other:identity:context".into();
    stale.content_sha256 = content_digest(&stale).unwrap();
    write_body(tmp.path(), &stale);
    crate::test_runner::lang_rust::witness_memo::clear_published_witness_memo_for_tests();
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert!(loaded.covered_lines.is_empty());
}

#[test]
fn incomplete_publication_writes_loadable_legacy_witness() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &sample_identity(),
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Failed],
        durations_ns: &[Some(1)],
        covered_lines: &Default::default(),
        complete: false,
        jobs: 1,
    })
    .unwrap();
    let loaded = load_witness_from_disk(tmp.path()).unwrap();
    assert!(!loaded.complete);
    assert_eq!(loaded.statuses, vec![WitnessStatus::Failed]);
}

#[test]
fn incomplete_publication_supersedes_older_complete_generation() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &Default::default(),
        complete: true,
        jobs: 1,
    })
    .unwrap();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Failed],
        durations_ns: &[Some(2)],
        covered_lines: &Default::default(),
        complete: false,
        jobs: 1,
    })
    .unwrap();
    crate::test_runner::lang_rust::witness_memo::clear_published_witness_memo_for_tests();
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert!(!loaded.complete);
    assert_eq!(loaded.statuses, vec![WitnessStatus::Failed]);
}

#[test]
fn broken_generation_pointer_does_not_fall_back_to_legacy_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &sample_identity(),
        scope: WitnessScope::Full,
        selectors: &["a".into()],
        statuses: &[WitnessStatus::Failed],
        durations_ns: &[Some(1)],
        covered_lines: &Default::default(),
        complete: false,
        jobs: 1,
    })
    .unwrap();
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let pointer = crate::test_runner::execution_generation::read_pointer(&cache_root)
        .unwrap()
        .unwrap();
    std::fs::remove_dir_all(cache_root.join("generations").join(pointer.generation_id)).unwrap();
    assert!(try_load_rust_execution_witness(tmp.path()).is_err());
}

#[test]
fn large_all_pass_witness_uses_logical_ids_for_cached_summary() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let selectors: Vec<String> = (0..65).map(|i| format!("tests::test_{i}")).collect();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &vec![WitnessStatus::Passed; selectors.len()],
        durations_ns: &vec![Some(1); selectors.len()],
        covered_lines: &Default::default(),
        complete: true,
        jobs: 1,
    })
    .unwrap();
    let summary = super::try_warm_rust_cached_summary(
        tmp.path(),
        &selectors,
        &identity,
        &kiss::GateConfig::default(),
    )
    .unwrap();
    assert_eq!(summary.total, selectors.len());
    assert!(summary.selector_durations_ns.contains_key("tests::test_0"));
}

#[test]
fn write_witness_atomic_reports_create_and_rename_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let file_root = tmp.path().join("not-a-dir");
    std::fs::write(&file_root, "x").unwrap();
    let body = disk_body("full", &["a"], &["passed"]);
    assert!(write_witness_atomic(&file_root, &body).is_err());
    write_minimal_repo(tmp.path());
    let cache = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("execution_witness.json");
    std::fs::create_dir_all(&cache).unwrap();
    assert!(write_witness_atomic(tmp.path(), &body).is_err());
}

#[test]
fn prune_returns_ok_when_cache_missing_or_empty() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let stale = "force_miss_batch_writes_warm_hit_seal_for_later_hit";
    let mut witness = ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec![stale.into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
        raw_statuses: vec![WitnessStatus::Passed],
    };
    prune_removed_rust_witness_selectors(tmp.path(), &mut witness).unwrap();
    assert_eq!(witness.selectors, vec![stale.to_string()]);
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        tmp.path(),
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    prune_removed_rust_witness_selectors(tmp.path(), &mut witness).unwrap();
    assert_eq!(witness.selectors, vec![stale.to_string()]);
}

#[test]
fn prune_keeps_witness_when_known_set_would_be_empty() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    crate::test_runner::workspace_selector_cache::store_workspace_selectors(
        tmp.path(),
        &[],
        &[],
        &["keep::one".into()],
        &[],
    )
    .unwrap();
    let stale = "force_miss_batch_writes_warm_hit_seal_for_later_hit";
    let mut witness = ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec![stale.into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::new(),
        complete: true,
        generation_id: "g".into(),
        raw_statuses: vec![WitnessStatus::Passed],
    };
    prune_removed_rust_witness_selectors(tmp.path(), &mut witness).unwrap();
    assert_eq!(witness.selectors, vec![stale.to_string()]);
}
