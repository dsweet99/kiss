//! Rust line coverage with conservative per-selector caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod batch_aggregate;
mod batch_check_aggregate;
mod batch_check_aggregate_export;
mod batch_check_aggregate_identity;
mod batch_derived;
mod batch_derived_entries;
mod batch_derived_incremental;
mod batch_derived_index;
mod batch_derived_index_check_aggregate_support;
mod batch_derived_index_types;
mod batch_derived_manifest;
mod batch_derived_prune;
mod batch_reverse_line_index;
mod batch_derived_snapshot;
mod batch_events;
mod batch_executable_index;
mod batch_executor;
mod batch_executor_finish;
mod batch_executor_finish_entries;
mod batch_executor_finish_export;
mod batch_executor_finish_store;
mod batch_executor_fresh;
mod batch_export;
mod batch_export_catalog;
mod batch_export_ignore;
mod batch_export_resolve;
mod batch_export_tools;
mod batch_fingerprint;
mod batch_identity_seal;
#[cfg(test)]
mod batch_identity_seal_test;
mod batch_lock;
mod batch_output_channel;
mod batch_output_channel_frame;
mod batch_output_channel_token;
mod batch_plan;
mod batch_plan_env;
mod batch_plan_nextest_config;
mod batch_plan_publish;
mod batch_plan_target_runner_program;
mod batch_plan_test_args;
mod batch_platform;
mod batch_process_tree;
mod batch_result;
mod batch_run;
mod batch_runner_resolve;
mod batch_nextest_id;
mod batch_shim;
mod batch_shim_synthesize;
#[cfg(unix)]
mod batch_shim_delegated;
mod batch_shim_lookup;
mod cargo_workspace_metadata;
mod file_lock;
mod kiss_tmp;
mod llvm_cov_json;
mod rust_cov_cache;
mod shared_input;
mod worker;

#[cfg(test)]
mod batch_derived_index_witness_test;
#[cfg(test)]
mod batch_export_contract_fixture;
#[cfg(test)]
#[path = "batch_export_contract_test.rs"]
mod batch_export_contract_test;
#[cfg(test)]
mod batch_plan_test;
#[cfg(test)]
mod lib_test;
#[cfg(test)]
mod rust_cov_cache_test;
#[cfg(test)]
mod shared_input_test;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod worker_cleanup_test;

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
pub use batch_reverse_line_index::{
    publish_reverse_line_index, query_reverse_line_index, reverse_line_index_dir,
};
pub use batch_derived_entries::{RustReusableSelectorEntry, load_reusable_prior_selector_entries};
pub use batch_derived_incremental::{IncrementalPublishPlan, publish_incremental_derived_state};
pub use batch_derived_index::{
    RustGenerationCoverageSnapshot, RustPopulationState, RustSnapshotDelta,
    is_check_aggregate_population, load_current_generation_coverage_snapshot,
    load_current_generation_line_index, load_current_population_state,
    load_reusable_prior_population_state, reusable_snapshot_delta,
};
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
pub use batch_shim::{TARGET_RUNNER_SHIM_SUBCOMMAND, run_target_runner_shim};
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

pub const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v3";
pub const BATCH_EXECUTION_POLICY_VERSION: &str = "rust-batch-execution-v1";

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
