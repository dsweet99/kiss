//! Env-gated bootstrap / debug for local acceptance.

use std::path::Path;

use super::{
    PublishRustWitness, publish_rust_execution_witness, rust_identity_digest_from_batch,
    rust_miss_selectors, try_load_rust_execution_witness, try_warm_rust_cached_summary,
};
use crate::test_runner::lang_iface::{WitnessScope, WitnessStatus};
use crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity;
use crate::test_runner::runners::enumerate_workspace_rust_selectors;

#[test]
fn bootstrap_repo_rust_witness_when_env_set() {
    if std::env::var("KISS_BOOTSTRAP_RUST_WITNESS").is_err() {
        return;
    }
    let root = Path::new(".");
    let identity = current_rust_coverage_batch_identity(root, &[]).expect("identity");
    let planned = enumerate_workspace_rust_selectors(root, &[]).expect("enumerate");
    eprintln!(
        "planned={} identity={}",
        planned.len(),
        rust_identity_digest_from_batch(&identity)
    );
    if let Ok(w) = try_load_rust_execution_witness(root) {
        eprintln!(
            "existing witness sel={} digest={} complete={}",
            w.selectors.len(),
            w.identity_digest,
            w.complete
        );
        let misses = rust_miss_selectors(root, &planned, &identity);
        eprintln!("misses={:?}", misses.as_ref().map(|m| m.len()));
        eprintln!(
            "warm={}",
            try_warm_rust_cached_summary(root, &planned, &identity).is_some()
        );
    }
    // Publish Full for *current planned* universe so All-mode can accept.
    let statuses = vec![WitnessStatus::Passed; planned.len()];
    let durations = vec![0u64; planned.len()];
    let empty_cov = Default::default();
    let id = publish_rust_execution_witness(PublishRustWitness {
        repo_root: root,
        identity: &identity,
        scope: WitnessScope::Full,
        selectors: &planned,
        statuses: &statuses,
        durations_ns: &durations,
        covered_lines: &empty_cov,
        complete: true,
    })
    .expect("publish");
    eprintln!("published {id} n={}", planned.len());
    assert!(try_warm_rust_cached_summary(root, &planned, &identity).is_some());
}
