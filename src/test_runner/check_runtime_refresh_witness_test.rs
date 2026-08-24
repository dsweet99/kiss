use super::super::{
    CheckAggregateRepairDecision, maybe_downgrade_rerun_when_witness_warm,
    retained_maps_ignoring_digest_mismatch,
};
use super::{aggregate_prior, aggregate_prior_with_maps};

#[test]
fn retained_maps_ignoring_digest_keeps_changed_binaries() {
    let selectors = vec!["pkg::bin$alpha".to_string()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "old"), ("bin-b", "stable")]);
    let mapped = std::collections::BTreeSet::from(["bin-a".to_string(), "bin-b".to_string()]);
    let maps = retained_maps_ignoring_digest_mismatch(&prior, &mapped);
    assert!(maps.contains_key("bin-a"));
    assert!(maps.contains_key("bin-b"));
}

#[test]
fn witness_warm_downgrade_leaves_non_rerun_decisions_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "gen".into(),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: Default::default(),
    };
    let selectors = vec!["a".into()];
    let prior = aggregate_prior(&selectors, &[("bin-a", "digest-a")]);
    let maps =
        std::collections::BTreeMap::from([(selectors[0].clone(), vec!["bin-a".to_string()])]);
    let identity_only = CheckAggregateRepairDecision::IdentityOnly {
        prior_generation: "g".into(),
        retained_binary_line_maps: Default::default(),
    };
    let out = maybe_downgrade_rerun_when_witness_warm(
        tmp.path(),
        &selectors,
        &identity,
        &prior,
        &maps,
        identity_only.clone(),
    );
    assert_eq!(out, identity_only);
}

#[test]
fn witness_absent_keeps_rerun_decision() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "gen".into(),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: Default::default(),
    };
    let selectors = vec!["a".into(), "b".into()];
    let prior = aggregate_prior_with_maps(
        &selectors,
        &[("bin-a", "old"), ("bin-b", "stable")],
        &[
            (selectors[0].as_str(), vec!["bin-a"]),
            (selectors[1].as_str(), vec!["bin-b"]),
        ],
    );
    let maps = std::collections::BTreeMap::from([
        (selectors[0].clone(), vec!["bin-a".to_string()]),
        (selectors[1].clone(), vec!["bin-b".to_string()]),
    ]);
    let rerun = CheckAggregateRepairDecision::Rerun {
        prior_generation: "g".into(),
        rerun_selectors: vec![selectors[0].clone()],
        replacement_binary_ids: std::collections::BTreeSet::from(["bin-a".to_string()]),
        retained_binary_line_maps: Default::default(),
    };
    let out = maybe_downgrade_rerun_when_witness_warm(
        tmp.path(),
        &selectors,
        &identity,
        &prior,
        &maps,
        rerun.clone(),
    );
    assert!(matches!(out, CheckAggregateRepairDecision::Rerun { .. }));
}

#[test]
fn witness_warm_downgrades_rerun_to_identity_only() {
    use crate::test_runner::execution_witness::{
        PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
    };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "gen".into(),
        selection_context_fingerprint: "sel".into(),
        ordinary_source_digests: Default::default(),
    };
    let selectors = vec!["a".into(), "b".into()];
    let empty_cov = Default::default();
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root: tmp.path(),
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
        durations_ns: &[Some(0), Some(0)],
        covered_lines: &empty_cov,
        complete: true,
    })
    .unwrap();
    let prior = aggregate_prior_with_maps(
        &selectors,
        &[("bin-a", "old"), ("bin-b", "stable")],
        &[
            (selectors[0].as_str(), vec!["bin-a"]),
            (selectors[1].as_str(), vec!["bin-b"]),
        ],
    );
    let maps = std::collections::BTreeMap::from([
        (selectors[0].clone(), vec!["bin-a".to_string()]),
        (selectors[1].clone(), vec!["bin-b".to_string()]),
    ]);
    let rerun = CheckAggregateRepairDecision::Rerun {
        prior_generation: "g".into(),
        rerun_selectors: vec![selectors[0].clone()],
        replacement_binary_ids: std::collections::BTreeSet::from(["bin-a".to_string()]),
        retained_binary_line_maps: Default::default(),
    };
    let out = maybe_downgrade_rerun_when_witness_warm(
        tmp.path(),
        &selectors,
        &identity,
        &prior,
        &maps,
        rerun,
    );
    match out {
        CheckAggregateRepairDecision::IdentityOnly {
            retained_binary_line_maps,
            ..
        } => {
            assert!(retained_binary_line_maps.contains_key("bin-a"));
            assert!(retained_binary_line_maps.contains_key("bin-b"));
        }
        other => panic!("expected IdentityOnly downgrade, got {other:?}"),
    }
}
