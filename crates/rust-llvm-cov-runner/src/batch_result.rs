use crate::{RustLlvmCovError, RustLlvmCovOutcome, RustTestBinaryIdentity};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RustCoverageBatchCounters {
    pub build_invocations: usize,
    pub test_instances: usize,
    pub export_jobs: usize,
    pub aggregate_binaries: usize,
    pub aggregate_exports: usize,
    pub cache_hits: usize,
    pub max_active_test_instances: usize,
    pub max_active_exports: usize,
    pub unmatched_selectors: usize,
    pub max_objects_per_export: usize,
    pub build_target_baseline_bytes: u64,
    pub export_phase_ms: u128,
    pub derived_state_published: bool,
    pub derived_repair: bool,
    pub entry_generation_count: usize,
    pub current_index_generation: String,
    pub cache_pruned_entries: usize,
    pub process_residual_count: usize,
    pub legacy_cleanup_deferred: bool,
}

#[derive(Debug)]
pub struct RustCoverageBatchResult {
    pub completed: Vec<RustLlvmCovOutcome>,
    pub batch_error: Option<RustLlvmCovError>,
    pub counters: RustCoverageBatchCounters,
    pub test_binaries: Vec<RustTestBinaryIdentity>,
}
