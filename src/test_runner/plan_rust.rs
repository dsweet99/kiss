
use crate::test_runner::execution_witness::{
    rust_identity_digest_from_batch, try_load_rust_execution_witness,
};
use crate::test_runner::lang_iface::{
    AcceptDecision, AcceptMode, accept_witness, reclassify_statuses_with_gate,
};
use kiss::GateConfig;

pub(super) fn rust_population_current_for_all_selectors(
    repo_root: &std::path::Path,
    selectors: &[String],
    gate: &GateConfig,
) -> bool {


    let cache_root =
        crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if !cache_root.join("population.json").is_file()
        && !cache_root.join("execution_witness.json").is_file()
    {
        return false;
    }
    let identity_started = std::time::Instant::now();
    let Ok(identity) =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(repo_root, &[])
    else {
        crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
        return false;
    };
    crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    if rust_llvm_cov_runner::load_current_population_state(
        &crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root),
        repo_root,
        &identity,
        Some(&expected),
    )
    .is_some()
    {
        return true;
    }


    rust_witness_accepts_full_universe(repo_root, &expected, &identity, gate)
}

fn rust_witness_accepts_full_universe(
    repo_root: &std::path::Path,
    selectors: &[String],
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    gate: &GateConfig,
) -> bool {
    let Ok(mut witness) = try_load_rust_execution_witness(repo_root) else {
        return false;
    };
    let current = rust_identity_digest_from_batch(identity);
    if witness.identity_digest != current || !witness.complete {
        return false;
    }
    witness.statuses = reclassify_statuses_with_gate(
        &witness.selectors,
        &witness.statuses,
        &witness.durations_ns,
        gate,
    );
    accept_witness(AcceptMode::All, selectors, &current, &witness) == AcceptDecision::Accept
}

#[cfg(test)]
mod tests {
    use super::{rust_population_current_for_all_selectors, rust_witness_accepts_full_universe};
    use crate::test_runner::execution_witness::{
        PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
        rust_identity_digest_from_batch,
    };
    use kiss::GateConfig;
    use std::collections::{BTreeMap, BTreeSet};

    fn identity() -> rust_llvm_cov_runner::RustCoverageBatchIdentity {
        rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        }
    }

    #[test]
    fn rust_witness_accept_helpers_fail_closed_without_witness() {
        let tmp = tempfile::tempdir().unwrap();
        let id = identity();
        let selectors = vec!["a".into()];
        let gate = GateConfig::default();
        assert!(!rust_witness_accepts_full_universe(tmp.path(), &selectors, &id, &gate));
        assert!(!rust_population_current_for_all_selectors(tmp.path(), &selectors, &gate));
    }

    #[test]
    fn rust_witness_accepts_complete_full_universe() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
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
        })
        .unwrap();
        assert!(!rust_identity_digest_from_batch(&id).is_empty());
        let gate = GateConfig::default();
        assert!(rust_witness_accepts_full_universe(tmp.path(), &selectors, &id, &gate));

        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &id,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Unresolved],
            durations_ns: &[Some(1), Some(1)],
            covered_lines: &covered,
            complete: false,
        })
        .unwrap();
        assert!(!rust_witness_accepts_full_universe(tmp.path(), &selectors, &id, &gate));
    }
}
