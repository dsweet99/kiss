//! Rust line coverage with conservative per-selector caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(unused_imports)]

mod plan;
mod execute_or_reuse;
mod publish_derived;

mod file_lock;
mod kiss_profraw;
mod rust_cov_cache;

#[cfg(test)]
mod lib_test;
#[cfg(test)]
mod rust_cov_cache_test;
#[cfg(test)]
mod structure_regression_test;
#[cfg(test)]
mod test_support;

// Crate-root aliases for moved modules (internal `crate::batch_*` / brace imports).
#[allow(unused_imports)]
pub(crate) use plan::batch_plan_shim_const;
pub(crate) use execute_or_reuse::batch_executable_index;
pub(crate) use plan::batch_fingerprint;
pub(crate) use plan::batch_identity_seal;
#[cfg(test)]
pub(crate) use plan::batch_identity_seal_test;
pub(crate) use plan::batch_plan;
pub(crate) use plan::batch_plan_env;
pub(crate) use plan::batch_plan_nextest_config;
pub(crate) use plan::batch_plan_publish;
pub(crate) use plan::batch_plan_target_runner_program;
pub(crate) use plan::batch_plan_test_args;
pub(crate) use plan::batch_platform;
pub(crate) use plan::batch_runner_resolve;
pub(crate) use plan::batch_nextest_id;
pub(crate) use plan::cargo_workspace_metadata;
pub(crate) use plan::shared_input;
#[cfg(test)]
pub(crate) use plan::batch_plan_test;
#[cfg(test)]
pub(crate) use plan::shared_input_test;
pub(crate) use execute_or_reuse::batch_aggregate;
pub(crate) use execute_or_reuse::batch_check_aggregate_export;
pub(crate) use execute_or_reuse::batch_events;
pub(crate) use execute_or_reuse::batch_executor;
pub(crate) use execute_or_reuse::batch_executor_finish;
pub(crate) use execute_or_reuse::batch_executor_finish_entries;
pub(crate) use execute_or_reuse::batch_executor_finish_export;
pub(crate) use execute_or_reuse::batch_executor_finish_store;
pub(crate) use execute_or_reuse::batch_executor_finish_bans;
pub(crate) use execute_or_reuse::batch_executor_fresh;
pub(crate) use execute_or_reuse::batch_export;
pub(crate) use execute_or_reuse::batch_export_catalog;
pub(crate) use execute_or_reuse::batch_export_ignore;
pub(crate) use execute_or_reuse::batch_export_resolve;
pub(crate) use execute_or_reuse::batch_export_tools;
pub(crate) use execute_or_reuse::batch_lock;
pub(crate) use execute_or_reuse::batch_output_channel;
pub(crate) use execute_or_reuse::batch_output_channel_frame;
pub(crate) use execute_or_reuse::batch_output_channel_token;
pub(crate) use execute_or_reuse::batch_process_tree;
pub(crate) use execute_or_reuse::batch_result;
pub(crate) use execute_or_reuse::batch_run;
pub(crate) use execute_or_reuse::batch_warm_hit_seal;
pub(crate) use execute_or_reuse::batch_shim;
pub(crate) use execute_or_reuse::batch_shim_synthesize;
#[cfg(unix)]
pub(crate) use execute_or_reuse::batch_shim_delegated;
pub(crate) use execute_or_reuse::batch_shim_lookup;
pub(crate) use execute_or_reuse::llvm_cov_json;
pub(crate) use execute_or_reuse::worker;
#[cfg(test)]
pub(crate) use execute_or_reuse::batch_export_contract_fixture;
#[cfg(test)]
pub(crate) use execute_or_reuse::batch_export_contract_test;
#[cfg(test)]
pub(crate) use execute_or_reuse::worker_cleanup_test;
pub(crate) use publish_derived::batch_check_aggregate;
pub(crate) use publish_derived::batch_check_aggregate_identity;
pub(crate) use publish_derived::batch_derived;
pub(crate) use publish_derived::batch_derived_entries;
pub(crate) use publish_derived::batch_derived_incremental;
pub(crate) use publish_derived::batch_derived_index;
pub(crate) use publish_derived::batch_derived_index_check_aggregate_support;
pub(crate) use publish_derived::batch_derived_index_reverse;
pub(crate) use publish_derived::batch_derived_index_types;
pub(crate) use publish_derived::batch_derived_index_write;
pub(crate) use publish_derived::batch_derived_manifest;
pub(crate) use publish_derived::batch_population_durations;
pub(crate) use publish_derived::batch_derived_prune;
pub(crate) use publish_derived::batch_entry_state;
pub(crate) use publish_derived::batch_reverse_build;
pub(crate) use publish_derived::batch_reverse_publish;
pub(crate) use publish_derived::batch_reverse_query;
pub(crate) use publish_derived::batch_reverse_query_metrics;
pub(crate) use publish_derived::batch_reverse_line_index;
#[cfg(test)]
pub(crate) use publish_derived::batch_reverse_test_support;
#[cfg(test)]
pub(crate) use publish_derived::batch_reverse_process_race_support;
pub(crate) use publish_derived::batch_derived_snapshot;
pub(crate) use publish_derived::batch_publication_tmp;
#[cfg(test)]
pub(crate) use publish_derived::batch_derived_index_witness_test;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub use batch_aggregate::{AggregationCounters, InstanceResult, aggregate_logical_selectors};
pub use batch_check_aggregate::{
    CHECK_AGGREGATE_SCHEMA_VERSION, CheckAggregateBinaryRecord, CheckAggregateSnapshot,
    ValidatedCheckAggregate, build_check_aggregate, load_current_check_aggregate_snapshot,
    load_reusable_prior_check_aggregate, publish_check_aggregate, reusable_check_aggregate_delta,
};
pub use batch_derived::{
    DerivedPublishCounters, INDEX_SCHEMA_VERSION as BATCH_INDEX_SCHEMA_VERSION,
    POPULATION_SCHEMA_VERSION as BATCH_POPULATION_SCHEMA_VERSION, population_derived_state_stale,
    population_manifest_state_is_current, prune_obsolete_selective_generations,
    publish_derived_state, publish_derived_state_with_binaries,
};
pub use batch_entry_state::{
    EntryState, ENTRY_STATE_SCHEMA, invalidate_entry_state, publish_next_entry_state,
    read_entry_state,
};
pub use batch_reverse_line_index::{
    ReversePublishInfo, REVERSE_LINE_INDEX_SCHEMA, prune_unreferenced_snapshots,
    publish_reverse_line_index, read_prior_snapshot_id, reverse_line_index_dir,
};
pub use batch_reverse_query::{
    query_reverse_line_index, snapshot_reverse_query_counters,
    take_reverse_query_counters_since_last_copy, ReverseQueryCounters, ReverseUnavailableCounts,
    REVERSE_QUERY_HITS,
};
pub use batch_reverse_query_metrics::ReverseUnavailableReason;
#[cfg(test)]
pub use batch_reverse_query::reset_reverse_query_counters_for_test;
pub use batch_derived_entries::{RustReusableSelectorEntry, load_reusable_prior_selector_entries};
pub use batch_derived_incremental::{
    IncrementalPublishPlan, publish_incremental_derived_state, rekey_selector_entries_to_identity,
};
pub use batch_derived_index::{
    RustGenerationCoverageSnapshot, RustPopulationState, RustSnapshotDelta,
    is_check_aggregate_population, load_current_generation_coverage_snapshot,
    load_current_generation_line_index, load_current_population_state,
    load_reusable_prior_population_state, reusable_snapshot_delta,
};
pub use batch_population_durations::load_current_population_durations;
pub use batch_events::{
    BatchCompilerArtifact, BatchEventStream, BatchTestTerminal, aggregate_selectors_for_test,
    parse_batch_event_stream, selector_matches_test,
};
pub use batch_executable_index::{RustTestExecutableIndex, build_rust_test_executable_index};
pub use batch_executor::execute_rust_coverage_batch;
#[cfg(test)]
pub use batch_export::FakeInstanceExporter;
pub use batch_export::{
    ExportCounters, InstanceExportRequest, SubprocessInstanceExporter, object_paths_for_executable,
};
pub use batch_export_catalog::{build_object_catalog, object_paths_from_artifacts};
pub use batch_export_resolve::BinaryIdObjectMap;
pub use batch_export_tools::{
    ExportTools, resolve_export_tools_from_env, resolve_export_tools_from_rustc,
};
pub use batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, batch_identity, entry_fingerprint,
};
pub use batch_plan::{
    CheckAggregateRepairPublication, CoverageOutputMode, RustCoverageBatchPlan,
    RustCoverageBatchRequest, build_rust_coverage_batch_plan, validate_supported_rust_cargo_args,
};
pub use batch_plan_publish::publish_generated_nextest_config;
pub use batch_plan_test_args::validate_supported_rust_test_args;
pub use batch_result::{RustCoverageBatchCounters, RustCoverageBatchResult};
pub use batch_runner_resolve::{
    DelegatedRunnerMap, RUNNER_RESOLVER_POLICY_VERSION, delegated_runner_for_platform,
    placeholder_delegated_runner_fields, read_runner_map, resolve_batch_request_runners,
    resolve_delegated_runners, runner_map_fingerprint, write_runner_map,
};
pub use plan::batch_plan_shim_const::TARGET_RUNNER_SHIM_SUBCOMMAND;
pub use batch_shim::run_target_runner_shim;
pub use kiss_profraw::{
    KissProfrawProcessGuard, discover_repo_root, redirect_this_process, sweep_kiss_profraw_dir,
};
pub use rust_cov_cache::{
    RustCovCacheEntry, generation_entries_fingerprint, repo_relative_coverage_file,
    repo_relative_path, store_rust_cov_cache_entry,
};
use serde::{Deserialize, Serialize};
pub use shared_input::{
    is_cargo_config_input_path, is_rust_cov_cache_input, rust_cov_input_files,
    selection_context_source_digest, workspace_input_digest,
};
pub use worker::rust_cov_cache_tmp_parent;

pub const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v4";
pub const BATCH_EXECUTION_POLICY_VERSION: &str = "rust-batch-execution-v2";

pub use llvm_cov_json::RustLineCoverage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustCovCacheStatus {
    Hit,
    MissStored,
    FreshUnstored,
}

#[cfg(test)]
impl RustCovCacheStatus {
    fn witness_hit() -> Self {
        Self::Hit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustLlvmCovOutcome {
    pub selector: String,
    pub status: rpytest_runner::TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub coverage: RustLineCoverage,
    pub test_binary_ids: Vec<String>,
    pub cache_status: RustCovCacheStatus,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustTestBinaryIdentity {
    pub id: String,
    pub executable: String,
    pub digest: String,
}

#[cfg(test)]
impl RustLlvmCovOutcome {
    fn witness() -> Self {
        Self {
            selector: "smoke::passes".to_string(),
            status: rpytest_runner::TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage::witness(),
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        }
    }
}

#[derive(Debug)]
pub enum RustLlvmCovError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidRequest(String),
    MissingArtifact(PathBuf),
    Interrupted,
}

impl From<io::Error> for RustLlvmCovError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RustLlvmCovError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
