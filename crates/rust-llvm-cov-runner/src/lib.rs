//! Rust line coverage with conservative per-selector caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod batch_aggregate;
mod batch_derived;
mod batch_derived_index;
mod batch_events;
mod batch_executor;
mod batch_executor_finish;
mod batch_executor_fresh;
mod batch_export;
mod batch_export_catalog;
mod batch_export_ignore;
mod batch_export_resolve;
mod batch_export_tools;
mod batch_fingerprint;
mod batch_lock;
mod batch_output_channel;
mod batch_output_channel_frame;
mod batch_output_channel_token;
mod batch_plan;
mod batch_plan_env;
mod batch_plan_nextest_config;
mod batch_plan_publish;
mod batch_plan_test_args;
mod batch_process_tree;
mod batch_result;
mod batch_run;
mod batch_runner_resolve;
mod batch_shim;
#[cfg(any(test, feature = "legacy-test-api"))]
mod cargo_runner;
mod file_lock;
#[cfg(any(test, feature = "legacy-test-api"))]
mod finalize;
mod llvm_cov_json;
mod rust_cov_cache;
mod shared_input;
mod worker;

#[cfg(test)]
mod batch_export_contract_fixture;
#[cfg(test)]
#[path = "batch_export_contract_test.rs"]
mod batch_export_contract_test;
#[cfg(test)]
mod batch_plan_test;
#[cfg(test)]
mod cargo_runner_test;
#[cfg(test)]
mod finalize_test;
#[cfg(test)]
mod lib_test;
#[cfg(test)]
mod lock_failure_test;
#[cfg(test)]
mod process_forced_race_test;
#[cfg(test)]
mod process_race_test;
#[cfg(test)]
mod rust_cov_cache_test;
#[cfg(test)]
mod shared_input_test;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod worker_cleanup_test;

#[cfg(any(test, feature = "legacy-test-api"))]
use std::collections::BTreeMap;
#[cfg(any(test, feature = "legacy-test-api"))]
use std::fs;
use std::io;
#[cfg(any(test, feature = "legacy-test-api"))]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub use batch_aggregate::{AggregationCounters, InstanceResult, aggregate_logical_selectors};
pub use batch_derived::{
    DerivedPublishCounters, INDEX_SCHEMA_VERSION as BATCH_INDEX_SCHEMA_VERSION,
    POPULATION_SCHEMA_VERSION as BATCH_POPULATION_SCHEMA_VERSION, population_derived_state_stale,
    population_manifest_state_is_current, publish_derived_state,
};
pub use batch_derived_index::{
    RustPopulationState, load_current_generation_line_index, load_current_population_state,
};
pub use batch_events::{
    BatchCompilerArtifact, BatchEventStream, BatchTestTerminal, aggregate_selectors_for_test,
    parse_batch_event_stream, selector_matches_test,
};
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
    RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan,
    validate_supported_rust_cargo_args,
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
#[cfg(any(test, feature = "legacy-test-api"))]
pub use cargo_runner::{
    CargoLlvmCovRunError, CargoLlvmCovRunOutcome, CargoLlvmCovRunRequest, CargoLlvmCovRunner,
    subprocess_cargo_llvm_cov_runner,
};
#[cfg(any(test, feature = "legacy-test-api"))]
use finalize::{finalize_run, rust_cov_artifact_path};
use rpytest_runner::TestStatus;
pub use rust_cov_cache::{
    RustCovCacheEntry, generation_entries_fingerprint, repo_relative_coverage_file,
    repo_relative_path, store_rust_cov_cache_entry,
};
#[cfg(any(test, feature = "legacy-test-api"))]
use rust_cov_cache::{load_rust_cov_cache_entry, rust_cov_fingerprint};
use serde::{Deserialize, Serialize};
pub use shared_input::{is_cargo_config_input_path, rust_cov_input_files, workspace_input_digest};
#[cfg(any(test, feature = "legacy-test-api"))]
use worker::{
    cleanup_legacy_worker_dirs, lock_legacy_cleanup, lock_selector, lock_worker,
    prepare_worker_slot, rust_cov_worker_slot_root, rust_cov_worker_tmp_root,
};

pub use worker::rust_cov_cache_tmp_parent;
#[cfg(any(test, feature = "legacy-test-api"))]
pub use worker::{RustWorkerCleanupReport, cleanup_surplus_rust_cov_worker_slots};

pub const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v2";
pub const BATCH_EXECUTION_POLICY_VERSION: &str = "rust-batch-execution-v1";

#[cfg(any(test, feature = "legacy-test-api"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustLlvmCovRequest {
    pub selector: String,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub cargo: PathBuf,
    pub llvm_cov_version: String,
    pub rustc_version: String,
    pub cargo_args: Vec<String>,
    pub test_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cache_root: PathBuf,
    pub force_rerun: bool,
    pub worker_slot: usize,
}

#[cfg(test)]
impl RustLlvmCovRequest {
    fn witness(root: &Path) -> Self {
        rust_cov_sample_request(root)
    }
}

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
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub coverage: RustLineCoverage,
    pub cache_status: RustCovCacheStatus,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

#[cfg(test)]
impl RustLlvmCovOutcome {
    fn witness() -> Self {
        Self {
            selector: "smoke::passes".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage::witness(),
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
    #[cfg(any(test, feature = "legacy-test-api"))]
    Runner(CargoLlvmCovRunError),
    InvalidRequest(String),
    MissingArtifact(PathBuf),
    Finalization(Vec<RustLlvmCovError>),
    Composite {
        primary: Box<RustLlvmCovError>,
        finalization: Vec<RustLlvmCovError>,
    },
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

#[cfg(any(test, feature = "legacy-test-api"))]
impl From<CargoLlvmCovRunError> for RustLlvmCovError {
    fn from(value: CargoLlvmCovRunError) -> Self {
        Self::Runner(value)
    }
}

#[cfg(any(test, feature = "legacy-test-api"))]
pub struct RustLlvmCov {
    runner: CargoLlvmCovRunner,
}

#[cfg(any(test, feature = "legacy-test-api"))]
impl RustLlvmCov {
    pub fn new(runner: CargoLlvmCovRunner) -> Self {
        Self { runner }
    }

    pub fn run_or_reuse(
        &self,
        req: RustLlvmCovRequest,
    ) -> Result<RustLlvmCovOutcome, RustLlvmCovError> {
        validate_rust_cov_request(&req)?;
        fs::create_dir_all(&req.cache_root)?;
        let fingerprint = rust_cov_fingerprint(&req)?;
        if !req.force_rerun
            && let Some(entry) = load_rust_cov_cache_entry(&req.cache_root, &fingerprint)
        {
            return Ok(rust_cov_outcome_from_cache(entry));
        }

        #[cfg(test)]
        worker::wait_at_unlocked_miss_hook()?;

        // Global lock order while the cache is live:
        // 1. selector lock;
        // 2. optional legacy-cleanup lock;
        // 3. worker-slot lock.
        // Surplus cleanup takes only a nonblocking worker-slot lock, and lock
        // files stay on disk because deleting live lock files can break mutual
        // exclusion across processes that already opened them.
        let _selector_guard = lock_selector(&req.cache_root, &fingerprint)?;
        if !req.force_rerun
            && let Some(entry) = load_rust_cov_cache_entry(&req.cache_root, &fingerprint)
        {
            return Ok(rust_cov_outcome_from_cache(entry));
        }
        {
            let _legacy_guard = lock_legacy_cleanup(&req.cache_root)?;
            cleanup_legacy_worker_dirs(&req.cache_root)?;
        }
        let _worker_guard = lock_worker(&req.cache_root, req.worker_slot)?;
        prepare_worker_slot(&req.cache_root, req.worker_slot)?;
        let artifact_path = rust_cov_artifact_path(&req.cache_root, &fingerprint);
        if let Some(parent) = artifact_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let run_req = build_cargo_runner_request(&req, &artifact_path);
        let run = self.runner.run_one(run_req).map_err(RustLlvmCovError::from);
        finalize_run(&req, &fingerprint, run)
    }
}

#[cfg(any(test, feature = "legacy-test-api"))]
pub fn build_llvm_cov_argv(req: &CargoLlvmCovRunRequest) -> Vec<String> {
    let mut argv = vec![
        req.cargo.to_string_lossy().to_string(),
        "llvm-cov".to_string(),
        "test".to_string(),
        "--json".to_string(),
        "--output-path".to_string(),
        req.artifact_path.to_string_lossy().to_string(),
        "--no-clean".to_string(),
    ];
    argv.extend(req.cargo_args.iter().cloned());
    argv.push(req.selector.clone());
    argv.push("--".to_string());
    argv.extend(req.test_args.iter().cloned());
    argv
}

#[cfg(any(test, feature = "legacy-test-api"))]
fn build_cargo_runner_request(
    req: &RustLlvmCovRequest,
    artifact_path: &Path,
) -> CargoLlvmCovRunRequest {
    let mut env = req.env.clone();
    let worker_root = rust_cov_worker_slot_root(&req.cache_root, req.worker_slot);
    env.insert(
        "CARGO_TARGET_DIR".to_string(),
        worker_root.join("target").to_string_lossy().to_string(),
    );
    env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        worker_root
            .join("profile")
            .join("%m-%p.profraw")
            .to_string_lossy()
            .to_string(),
    );
    env.insert(
        "TMPDIR".to_string(),
        rust_cov_worker_tmp_root(&req.cache_root, req.worker_slot)
            .to_string_lossy()
            .to_string(),
    );
    CargoLlvmCovRunRequest {
        selector: req.selector.clone(),
        cwd: req.cwd.clone(),
        cargo: req.cargo.clone(),
        cargo_args: req.cargo_args.clone(),
        test_args: req.test_args.clone(),
        env,
        artifact_path: artifact_path.to_path_buf(),
    }
}

#[cfg(any(test, feature = "legacy-test-api"))]
fn rust_cov_outcome_from_cache(entry: RustCovCacheEntry) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: entry.selector,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        cache_status: RustCovCacheStatus::Hit,
        stdout: None,
        stderr: None,
    }
}

#[cfg(any(test, feature = "legacy-test-api"))]
fn validate_rust_cov_request(req: &RustLlvmCovRequest) -> Result<(), RustLlvmCovError> {
    if req.selector.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "rust test selector must not be empty".to_string(),
        ));
    }
    if req.llvm_cov_version.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "cargo llvm-cov version must be part of the cache key".to_string(),
        ));
    }
    if req.rustc_version.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "rustc version must be part of the cache key".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn rust_cov_sample_request(root: &Path) -> RustLlvmCovRequest {
    RustLlvmCovRequest {
        selector: "smoke::passes".to_string(),
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
        rustc_version: "rustc 1.88.0".to_string(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: vec!["--nocapture".to_string()],
        env: BTreeMap::new(),
        cache_root: root.join(".rust_llvm_cov_cache"),
        force_rerun: false,
        worker_slot: 0,
    }
}
