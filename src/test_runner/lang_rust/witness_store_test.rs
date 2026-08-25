use std::path::Path;

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::{
    PublishRustWitness, RustWarmDecision, maybe_bootstrap_rust_witness,
    publish_rust_execution_witness, rust_identity_digest_from_batch, rust_miss_selectors,
    rust_warm_or_miss_selectors, try_load_rust_execution_witness, try_warm_rust_cached_summary,
};
use crate::test_runner::lang_iface::{WitnessScope, WitnessStatus};

fn sample_identity() -> RustCoverageBatchIdentity {
    RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "gen".into(),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: Default::default(),
    }
}

fn write_minimal_repo(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

#[test]
fn publish_load_round_trip_and_warm_accept() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let selectors = vec!["a".into(), "b".into()];
    let statuses = vec![WitnessStatus::Passed, WitnessStatus::Passed];
    let durations = vec![Some(10), Some(20)];
    let empty_cov = Default::default();
    let id = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &statuses,
        durations_ns: &durations,
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    assert!(id.starts_with("rust-wit-"));
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(loaded.selectors, selectors);
    assert_eq!(loaded.statuses, statuses);
    assert_eq!(loaded.durations_ns, durations);
    assert!(loaded.complete);
    assert_eq!(
        loaded.identity_digest,
        rust_identity_digest_from_batch(&identity)
    );
    assert!(
        try_warm_rust_cached_summary(
            tmp.path(),
            &selectors,
            &identity,
            &kiss::GateConfig::default()
        )
        .is_some()
    );
    assert_eq!(
        rust_miss_selectors(
            tmp.path(),
            &["a".into(), "c".into()],
            &identity,
            &kiss::GateConfig::default()
        ),
        Some(vec!["c".into()])
    );
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into(), "c".into()],
        &identity,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::RunMisses(misses) => assert_eq!(misses, vec!["c".to_string()]),
        other => panic!("expected RunMisses, got {other:?}"),
    }
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &selectors,
        &identity,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Warm(_) => {}
        other => panic!("expected Warm, got {other:?}"),
    }
    maybe_bootstrap_rust_witness(tmp.path(), &selectors, &identity);
}

#[test]
fn subset_scope_does_not_overwrite_full_pointer() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let full_sels = vec!["a".into()];
    let subset_sels = vec!["b".into()];
    let empty_cov = Default::default();
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &full_sels,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    let before = try_load_rust_execution_witness(tmp.path()).unwrap();
    let empty = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Subset,
        selectors: &subset_sels,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: false,
    })
    .unwrap();
    assert!(empty.is_empty());
    let after = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(before.generation_id, after.generation_id);
    assert_eq!(after.selectors, vec!["a".to_string()]);
}

#[test]
fn checksum_mismatch_rejects_load() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let sels = vec!["a".into()];
    let empty_cov = Default::default();
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &sels,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    let path = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("execution_witness.json");
    let mut raw = std::fs::read_to_string(&path).unwrap();
    raw = raw.replace("\"complete\": true", "\"complete\": false");
    std::fs::write(&path, raw).unwrap();
    assert!(try_load_rust_execution_witness(tmp.path()).is_err());
}

#[test]
fn warm_helpers_use_caller_gate_not_cwd_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let selectors = vec!["a".into()];
    let empty_cov = Default::default();

    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(2_000_000_000)],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    let loose = kiss::GateConfig {
        max_unit_test_seconds: vec![("*".into(), 3600.0)],
        ..kiss::GateConfig::default()
    };
    let tight = kiss::GateConfig {
        max_unit_test_seconds: vec![("*".into(), 0.5)],
        ..kiss::GateConfig::default()
    };
    assert!(
        try_warm_rust_cached_summary(tmp.path(), &selectors, &identity, &loose).is_some(),
        "loose session gate must warm-accept"
    );
    assert!(
        try_warm_rust_cached_summary(tmp.path(), &selectors, &identity, &tight).is_none(),
        "tight session gate must reject the same witness"
    );
    assert_eq!(
        rust_miss_selectors(tmp.path(), &selectors, &identity, &tight),
        Some(vec!["a".into()])
    );
}
