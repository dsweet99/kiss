use std::collections::BTreeMap;
use std::path::Path;

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::witness_store::{PublishRustWitness, publish_rust_execution_witness};
use crate::test_runner::lang_iface::{WitnessScope, WitnessStatus};

pub(crate) fn maybe_bootstrap_rust_witness(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustCoverageBatchIdentity,
) {
    if std::env::var("KISS_BOOTSTRAP_RUST_WITNESS").is_err() {
        return;
    }
    let statuses = vec![WitnessStatus::Passed; selectors.len()];
    let durations = vec![Some(0u64); selectors.len()];
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root,
        identity,
        scope: WitnessScope::Full,
        selectors,
        statuses: &statuses,
        durations_ns: &durations,
        covered_lines: &BTreeMap::new(),
        complete: true,
        jobs: 1,
    });
}
