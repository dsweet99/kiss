use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::{
    PublishRustWitness, RustWarmDecision, publish_rust_execution_witness,
    rust_warm_or_miss_selectors,
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

fn write_minimal_repo(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

fn publish_ab(root: &std::path::Path, identity: &RustCoverageBatchIdentity, complete: bool) {
    let selectors = vec!["a".into(), "b".into()];
    let empty_cov = Default::default();
    publish_rust_execution_witness(PublishRustWitness {
        repo_root: root,
        identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
        durations_ns: &[Some(10), Some(20)],
        covered_lines: &empty_cov,
        complete,
        jobs: 1,
    })
    .unwrap();
}

#[test]
fn rust_warm_reuses_witness_when_generation_drifts() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    publish_ab(tmp.path(), &identity, true);
    let drifted = RustCoverageBatchIdentity {
        input_digest: identity.input_digest.clone(),
        generation_fingerprint: "gen-other".into(),
        selection_context_fingerprint: "sel-other".into(),
        ordinary_source_digests: Default::default(),
    };
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into(), "b".into()],
        &drifted,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Warm(summary) => assert_eq!(summary.total, 2),
        other => panic!("source-stable generation drift must stay warm, got {other:?}"),
    }
}

#[test]
fn rust_warm_misses_when_ordinary_source_bytes_change() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let before = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &[],
    )
    .unwrap();
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    kiss::rust_llvm_cov_runner::write_ordinary_source_snapshot(&cache_root, tmp.path(), &before)
        .unwrap();
    publish_ab(tmp.path(), &before, true);
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    kiss::rust_llvm_cov_runner::refresh_identity_memo();
    let after = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &[],
    )
    .unwrap();
    assert_eq!(
        before.input_digest, after.input_digest,
        "ordinary source bytes must not change input_digest"
    );
    assert_ne!(
        before.ordinary_source_digests, after.ordinary_source_digests,
        "ordinary source bytes must change file digests"
    );
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into(), "b".into()],
        &after,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Miss | RustWarmDecision::RunMisses(_) => {}
        other => panic!("ordinary rust source edit must not stay warm, got {other:?}"),
    }
}

#[test]
fn rust_warm_misses_when_source_digest_changes() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    publish_ab(tmp.path(), &identity, true);
    let other = RustCoverageBatchIdentity {
        input_digest: "other-input".into(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        ordinary_source_digests: Default::default(),
    };
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into(), "b".into()],
        &other,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Miss => {}
        other => panic!("source digest change must miss, got {other:?}"),
    }
}

#[test]
fn rust_warm_reuses_incomplete_full_universe_witness() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let identity = sample_identity();
    publish_ab(tmp.path(), &identity, false);
    match rust_warm_or_miss_selectors(
        tmp.path(),
        &["a".into(), "b".into()],
        &identity,
        &kiss::GateConfig::default(),
    ) {
        RustWarmDecision::Warm(summary) => assert_eq!(summary.total, 2),
        other => panic!("incomplete full-universe witness must stay warm, got {other:?}"),
    }
}
