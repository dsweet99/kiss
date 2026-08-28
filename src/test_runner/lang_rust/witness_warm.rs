use std::path::Path;

use kiss::GateConfig;
use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::witness_store::{rust_miss_selectors, try_warm_rust_cached_summary};
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) fn rust_warm_or_miss_selectors(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
    gate: &GateConfig,
) -> RustWarmDecision {
    if let Some(summary) =
        try_warm_rust_cached_summary(repo_root, planned_selectors, identity, gate)
    {
        return RustWarmDecision::Warm(Box::new(summary));
    }
    match rust_miss_selectors(repo_root, planned_selectors, identity, gate) {
        Some(misses) if !misses.is_empty() && misses.len() < planned_selectors.len() => {
            RustWarmDecision::RunMisses(misses)
        }
        _ => RustWarmDecision::Miss,
    }
}

#[derive(Debug)]
pub(crate) enum RustWarmDecision {
    Warm(Box<SelectorExecutionSummary>),
    RunMisses(Vec<String>),
    Miss,
}
