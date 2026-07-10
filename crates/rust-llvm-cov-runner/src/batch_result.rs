use crate::{RustLlvmCovError, RustLlvmCovOutcome};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RustCoverageBatchCounters {
    pub build_invocations: usize,
    pub test_instances: usize,
    pub export_jobs: usize,
    pub cache_hits: usize,
    pub max_active_test_instances: usize,
    pub max_active_exports: usize,
}

#[derive(Debug)]
pub struct RustCoverageBatchResult {
    pub completed: Vec<RustLlvmCovOutcome>,
    pub batch_error: Option<RustLlvmCovError>,
    pub counters: RustCoverageBatchCounters,
}
