use std::collections::BTreeSet;
use std::path::Path;

use kiss::rust_llvm_cov_runner::{OrdinarySourceInvalidation, RustCoverageBatchIdentity};

use super::{
    PublishRustWitness, RustWarmDecision, apply_warm_invalidation, maybe_bootstrap_rust_witness,
    planned_misses_for, publish_rust_execution_witness, rust_identity_digest_from_batch,
    rust_miss_selectors, rust_source_delta_misses, rust_warm_or_miss_selectors,
    try_load_rust_execution_witness, try_warm_rust_cached_summary,
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

fn cover_delta_generation_and_bootstrap(root: &Path, identity: &RustCoverageBatchIdentity) {
    let planned = vec!["a".into(), "b".into()];
    let _ = rust_source_delta_misses(root, &planned, &[]);
    assert_eq!(
        planned_misses_for(&planned, OrdinarySourceInvalidation::All),
        planned
    );
    let only_a = BTreeSet::from(["a".into()]);
    assert_eq!(
        planned_misses_for(&planned, OrdinarySourceInvalidation::Selectors(only_a)),
        vec!["a".to_string()]
    );
    assert!(planned_misses_for(&planned, OrdinarySourceInvalidation::None).is_empty());
    assert!(super::generation_publish::try_load_full_generation_witness(root).is_some());
    let _ = super::generation_publish::publish_complete_full_generation(
        root,
        "ctx",
        &["a".into()],
        &[WitnessStatus::Failed],
        &[Some(1)],
        "timing",
        &Default::default(),
    );
    let _guard = crate::cwd_test_lock::lock();
    struct ClearEnv;
    impl Drop for ClearEnv {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("KISS_BOOTSTRAP_RUST_WITNESS");
            }
        }
    }
    let _clear = ClearEnv;
    unsafe {
        std::env::set_var("KISS_BOOTSTRAP_RUST_WITNESS", "1");
    }
    maybe_bootstrap_rust_witness(root, &planned, identity);
}

fn cover_warm_selector_invalidation(root: &Path, identity: &RustCoverageBatchIdentity) {
    let planned = vec!["a".into(), "b".into()];
    let gate = kiss::GateConfig::default();
    match apply_warm_invalidation(
        root,
        &planned,
        identity,
        &gate,
        OrdinarySourceInvalidation::Selectors(BTreeSet::from(["a".into()])),
    ) {
        RustWarmDecision::RunMisses(misses) => assert_eq!(misses, vec!["a".to_string()]),
        other => panic!("expected RunMisses, got {other:?}"),
    }
    match apply_warm_invalidation(
        root,
        &planned,
        identity,
        &gate,
        OrdinarySourceInvalidation::Selectors(BTreeSet::from(["outside".into()])),
    ) {
        RustWarmDecision::Warm(_) => {}
        other => panic!("unaffected planned selectors should stay warm, got {other:?}"),
    }
    match apply_warm_invalidation(
        root,
        &planned,
        identity,
        &gate,
        OrdinarySourceInvalidation::All,
    ) {
        RustWarmDecision::Miss => {}
        other => panic!("expected Miss, got {other:?}"),
    }
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
        jobs: 1,
    })
    .unwrap();
    assert!(!id.is_empty());
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(loaded.generation_id, id);
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
        RustWarmDecision::Miss => {}
        other => panic!("expected Miss without binary authority, got {other:?}"),
    }
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &selectors,
        &identity,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Miss => {}
        other => panic!("expected Miss without binary authority, got {other:?}"),
    }
    maybe_bootstrap_rust_witness(tmp.path(), &selectors, &identity);
    cover_warm_selector_invalidation(tmp.path(), &identity);
    cover_delta_generation_and_bootstrap(tmp.path(), &identity);
}

#[test]
fn maybe_bootstrap_writes_witness_when_env_set() {
    let _guard = crate::cwd_test_lock::lock();
    struct ClearEnv;
    impl Drop for ClearEnv {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("KISS_BOOTSTRAP_RUST_WITNESS");
            }
        }
    }
    let _clear = ClearEnv;
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    unsafe {
        std::env::set_var("KISS_BOOTSTRAP_RUST_WITNESS", "1");
    }
    maybe_bootstrap_rust_witness(tmp.path(), &["a".into()], &sample_identity());
    assert!(try_load_rust_execution_witness(tmp.path()).is_ok());
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
        jobs: 1,
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
        jobs: 1,
    })
    .unwrap();
    assert!(empty.is_empty());
    let after = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(before.generation_id, after.generation_id);
    assert_eq!(after.selectors, vec!["a".to_string()]);
}

#[test]
fn corrupted_legacy_sidecar_cannot_override_generation() {
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
        jobs: 1,
    })
    .unwrap();
    let path = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("execution_witness.json");
    assert!(
        !path.exists(),
        "successful Full generation must not write execution_witness.json"
    );
    let from_generation = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert!(
        from_generation.complete,
        "Full generation must load without a legacy witness file"
    );
    assert_eq!(from_generation.raw_statuses, vec![WitnessStatus::Passed]);

    let json_only = tempfile::tempdir().unwrap();
    write_minimal_repo(json_only.path());
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root: json_only.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &sels,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: false,
        jobs: 1,
    })
    .unwrap();
    let json_path = json_only
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("execution_witness.json");
    let mut json_raw = std::fs::read_to_string(&json_path).unwrap();
    json_raw = json_raw.replace("\"complete\": false", "\"complete\": true");
    std::fs::write(&json_path, json_raw).unwrap();
    let authoritative = try_load_rust_execution_witness(json_only.path()).unwrap();
    assert!(!authoritative.complete);
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
        jobs: 1,
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
        try_warm_rust_cached_summary(tmp.path(), &selectors, &identity, &tight).is_some(),
        "tight session gate reports a cached violation without rerunning"
    );
    assert_eq!(
        rust_miss_selectors(tmp.path(), &selectors, &identity, &tight),
        Some(Vec::<String>::new())
    );
}

#[test]
fn pointer_backed_witness_load_populates_in_process_memo() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let selectors = vec!["a".into()];
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &Default::default(),
        complete: true,
        jobs: 1,
    })
    .unwrap();
    super::witness_memo::clear_published_witness_memo_for_tests();
    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    crate::test_runner::execution_generation::reset_load_current_generation_call_count(&cache);
    let first = try_load_rust_execution_witness(tmp.path()).unwrap();
    let second = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(second.generation_id, first.generation_id);
    assert_eq!(
        crate::test_runner::execution_generation::load_current_generation_call_count(),
        1
    );
    let pointer = crate::test_runner::execution_generation::read_pointer(&cache)
        .unwrap()
        .unwrap();
    std::fs::remove_file(cache.join("current_generation.json")).unwrap();
    std::fs::remove_dir_all(cache.join("generations").join(pointer.generation_id)).unwrap();
    assert!(
        try_load_rust_execution_witness(tmp.path()).is_err(),
        "a pointer-backed memo must not outlive its generation authority"
    );
}

#[test]
fn publish_rejects_shape_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let empty_cov = Default::default();
    let err = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &sample_identity(),
        scope: WitnessScope::Full,
        selectors: &["a".into(), "b".into()],
        statuses: &[WitnessStatus::Passed],
        durations_ns: &[Some(1)],
        covered_lines: &empty_cov,
        complete: true,
        jobs: 1,
    });
    assert!(err.is_err());
}

#[test]
fn publish_dedups_duplicate_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let empty_cov = Default::default();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &["b".into(), "a".into(), "a".into()],
        statuses: &[
            WitnessStatus::Passed,
            WitnessStatus::Failed,
            WitnessStatus::Passed,
        ],
        durations_ns: &[Some(1), Some(2), Some(3)],
        covered_lines: &empty_cov,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    let loaded = try_load_rust_execution_witness(tmp.path()).unwrap();
    assert_eq!(loaded.selectors, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn rust_miss_and_warm_are_none_without_witness() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let gate = kiss::GateConfig::default();
    assert!(rust_miss_selectors(tmp.path(), &["a".into()], &identity, &gate).is_none());
    assert!(try_warm_rust_cached_summary(tmp.path(), &["a".into()], &identity, &gate).is_none());
}

#[test]
fn rust_miss_is_none_when_identity_drifts() {
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
    let mut drifted = identity.clone();
    drifted.input_digest = "other-input".into();
    assert!(
        rust_miss_selectors(
            tmp.path(),
            &["a".into()],
            &drifted,
            &kiss::GateConfig::default()
        )
        .is_none()
    );
}

#[test]
fn rust_miss_repairs_raw_status_len_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    let path = super::witness_store::witness_path(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "x").unwrap();
    super::witness_memo::stash_published_witness(
        tmp.path(),
        &path,
        crate::test_runner::lang_iface::ExecutionWitness {
            language: "rust".into(),
            scope: WitnessScope::Full,
            identity_digest: rust_identity_digest_from_batch(&identity),
            selectors: vec!["a".into()],
            statuses: vec![WitnessStatus::Passed],
            durations_ns: vec![Some(1)],
            covered_lines: Default::default(),
            complete: true,
            generation_id: "g".into(),
            raw_statuses: Vec::new(),
        },
    );
    let gate = kiss::GateConfig::default();
    assert_eq!(
        rust_miss_selectors(tmp.path(), &["a".into()], &identity, &gate),
        Some(Vec::<String>::new())
    );
    let _ = try_warm_rust_cached_summary(tmp.path(), &["a".into()], &identity, &gate);
}

#[test]
fn rust_warm_or_miss_is_miss_without_witness() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into()],
        &sample_identity(),
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Miss => {}
        other => panic!("expected Miss, got {other:?}"),
    }
}

#[test]
fn warm_rejects_when_generation_drifts_with_same_input() {
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
        durations_ns: &[Some(10)],
        covered_lines: &empty_cov,
        complete: true,
        jobs: 1,
    })
    .unwrap();
    let mut drifted = identity.clone();
    drifted.generation_fingerprint = "other-gen".into();
    drifted.selection_context_fingerprint = "other-sel".into();
    assert!(
        try_warm_rust_cached_summary(
            tmp.path(),
            &selectors,
            &drifted,
            &kiss::GateConfig::default()
        )
        .is_none()
    );
}
