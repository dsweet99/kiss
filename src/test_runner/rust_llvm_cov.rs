use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchRequest, RustCoverageBatchResult,
    RustCoverageToolIdentity, RustLlvmCovError, RustLlvmCovOutcome, build_rust_coverage_batch_plan,
    execute_rust_coverage_batch, resolve_batch_request_runners, validate_supported_rust_test_args,
};

#[cfg(test)]
mod rust_llvm_cov_request {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use rust_llvm_cov_runner::{RustCoverageBatchRequest, validate_supported_rust_test_args};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct RustLlvmCovRequest {
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

    pub(crate) fn rust_llvm_cov_request_from_parts(
        repo_root: &std::path::Path,
        selector: &str,
        extra: &[String],
        llvm_cov_version: &str,
        rustc_version: &str,
        force_rerun: bool,
    ) -> Result<RustLlvmCovRequest, String> {
        validate_supported_rust_test_args(extra)?;
        Ok(RustLlvmCovRequest {
            selector: selector.to_string(),
            cwd: repo_root.to_path_buf(),
            source_root: repo_root.to_path_buf(),
            cargo: PathBuf::from("cargo"),
            llvm_cov_version: llvm_cov_version.to_string(),
            rustc_version: rustc_version.to_string(),
            cargo_args: Vec::new(),
            test_args: extra.to_vec(),
            env: Default::default(),
            cache_root: repo_root.join(".kiss").join("rust_llvm_cov_cache"),
            force_rerun,
            worker_slot: 0,
        })
    }

    pub(crate) fn rust_llvm_cov_request_from_batch_parts(
        batch_req: &RustCoverageBatchRequest,
        selector: &str,
        llvm_cov_version: &str,
        rustc_version: &str,
    ) -> Result<RustLlvmCovRequest, String> {
        Ok(RustLlvmCovRequest {
            selector: selector.to_string(),
            cwd: batch_req.cwd.clone(),
            source_root: batch_req.source_root.clone(),
            cargo: batch_req.cargo.clone(),
            llvm_cov_version: llvm_cov_version.to_string(),
            rustc_version: rustc_version.to_string(),
            cargo_args: batch_req.cargo_args.clone(),
            test_args: batch_req.test_args.clone(),
            env: batch_req.env.clone(),
            cache_root: batch_req.cache_root.clone(),
            force_rerun: batch_req.force_rerun,
            worker_slot: 0,
        })
    }
}

#[cfg(test)]
pub(crate) use rust_llvm_cov_request::{
    rust_llvm_cov_request_from_batch_parts, rust_llvm_cov_request_from_parts,
};

use super::last_status::{LastStatusIdentity, record_statuses, rust_last_status_identity};
use super::runners::{SelectorCacheRecord, SelectorExecutionSummary, command_stdout};
use crate::test_runner::rust_coverage_index::relevant_rust_batch_env;

pub(crate) fn run_rust_llvm_cov_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        extra,
        force_rerun,
        jobs,
        population_publication_selectors,
        detect_rust_coverage_tool_versions,
        execute_rust_coverage_batch_compat,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RustCoverageToolVersions {
    cargo: String,
    llvm_cov: String,
    rustc: String,
    cargo_nextest: String,
}

#[allow(clippy::too_many_arguments)]
fn run_rust_llvm_cov_selectors_with_deps<D, E>(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    detect_versions: D,
    execute_batch: E,
) -> Result<SelectorExecutionSummary, String>
where
    D: FnOnce(&Path) -> Result<RustCoverageToolVersions, String>,
    E: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolVersions,
    ) -> Result<RustCoverageBatchResult, String>,
{
    assert!(jobs > 0, "jobs must be greater than zero");
    validate_supported_rust_test_args(extra)?;
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let batch_req = rust_coverage_batch_request_from_parts(
        repo_root,
        selectors,
        extra,
        force_rerun,
        jobs,
        population_publication_selectors,
    )?;
    build_rust_coverage_batch_plan(&batch_req)?;
    let versions = detect_versions(repo_root)?;
    let identity = rust_last_status_identity(
        &versions.cargo,
        &versions.llvm_cov,
        &versions.rustc,
        &versions.cargo_nextest,
        extra,
        &batch_req.runner_map_fingerprint,
    );
    let result = execute_batch(&batch_req, &versions)?;
    finish_rust_coverage_batch_result(repo_root, &identity, result)
}

fn execute_rust_coverage_batch_compat(
    batch_req: &RustCoverageBatchRequest,
    versions: &RustCoverageToolVersions,
) -> Result<RustCoverageBatchResult, String> {
    let tools = RustCoverageToolIdentity {
        cargo_version: versions.cargo.clone(),
        llvm_cov_version: versions.llvm_cov.clone(),
        rustc_version: versions.rustc.clone(),
        cargo_nextest_version: versions.cargo_nextest.clone(),
    };
    execute_rust_coverage_batch(batch_req, &tools).map_err(format_rust_llvm_cov_error)
}

pub(crate) fn rust_coverage_batch_request_from_parts(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
) -> Result<RustCoverageBatchRequest, String> {
    validate_supported_rust_test_args(extra)?;
    let mut req = RustCoverageBatchRequest {
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo_root.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: Vec::new(),
        test_args: extra.to_vec(),
        env: relevant_rust_batch_env(),
        force_rerun,
        jobs,
        generated_config: unique_rust_coverage_batch_config_path(repo_root),
        population_publication_selectors,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
    };
    resolve_batch_request_runners(&mut req).map_err(format_rust_llvm_cov_error)?;
    Ok(req)
}

fn unique_rust_coverage_batch_config_path(repo_root: &Path) -> PathBuf {
    static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(0);
    let run_id = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    repo_root
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join(format!(
            "run-{}-{timestamp_nanos}-{run_id}",
            std::process::id()
        ))
        .join("nextest.toml")
}

fn detect_rust_coverage_tool_versions(
    repo_root: &Path,
) -> Result<RustCoverageToolVersions, String> {
    let cargo = PathBuf::from("cargo");
    let cargo_version = command_stdout(&cargo, &["--version"], repo_root)?;
    let llvm_cov_version = command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?;
    let cargo_nextest_version = command_stdout(&cargo, &["nextest", "--version"], repo_root)?;
    let rustc = PathBuf::from("rustc");
    let rustc_version = command_stdout(&rustc, &["-Vv"], repo_root)?;
    Ok(RustCoverageToolVersions {
        cargo: cargo_version,
        llvm_cov: llvm_cov_version,
        rustc: rustc_version,
        cargo_nextest: cargo_nextest_version,
    })
}

fn print_rust_llvm_cov_outcome(outcome: &RustLlvmCovOutcome) {
    match (outcome.status, outcome.cache_status) {
        (rpytest_runner::TestStatus::Passed, RustCovCacheStatus::Hit) => {
            println!("PASSED (cached): {}", outcome.selector);
        }
        (rpytest_runner::TestStatus::Passed, RustCovCacheStatus::MissStored) => {
            println!("PASSED: {}", outcome.selector);
        }
        (rpytest_runner::TestStatus::Passed, RustCovCacheStatus::FreshUnstored) => {
            println!("PASSED (not cached): {}", outcome.selector);
        }
        (rpytest_runner::TestStatus::Failed, RustCovCacheStatus::Hit) => {
            println!("FAILED (cached): {}", outcome.selector);
            eprintln!(
                "Failure output was not cached. Re-run with --force to reproduce stdout/stderr."
            );
        }
        (rpytest_runner::TestStatus::Failed, RustCovCacheStatus::MissStored) => {
            println!("FAILED: {}", outcome.selector);
            if let Some(stderr) = &outcome.stderr
                && !stderr.is_empty()
            {
                eprint!("{}", String::from_utf8_lossy(stderr));
            }
        }
        (rpytest_runner::TestStatus::Failed, RustCovCacheStatus::FreshUnstored) => {
            println!("FAILED (not cached): {}", outcome.selector);
            if let Some(stderr) = &outcome.stderr
                && !stderr.is_empty()
            {
                eprint!("{}", String::from_utf8_lossy(stderr));
            }
        }
    }
}

fn finish_rust_coverage_batch_result(
    repo_root: &Path,
    identity: &LastStatusIdentity,
    result: RustCoverageBatchResult,
) -> Result<SelectorExecutionSummary, String> {
    let mut summary = SelectorExecutionSummary::default();
    summary.record_rust_batch_counters(&result.counters);
    let mut statuses = Vec::new();
    for outcome in &result.completed {
        print_rust_llvm_cov_outcome(outcome);
        statuses.push((outcome.selector.clone(), outcome.status));
        let cache_record = match outcome.cache_status {
            RustCovCacheStatus::Hit => SelectorCacheRecord::Hit,
            RustCovCacheStatus::MissStored => SelectorCacheRecord::MissStored,
            RustCovCacheStatus::FreshUnstored => SelectorCacheRecord::MissUnstored,
        };
        summary.record(outcome.status, cache_record, outcome.exit_code);
    }
    record_statuses(repo_root, kiss::Language::Rust, identity, &statuses)?;
    if let Some(err) = result.batch_error {
        return Err(format_rust_llvm_cov_error(err));
    }
    Ok(summary)
}

fn format_rust_llvm_cov_error(err: RustLlvmCovError) -> String {
    format!("error: kiss test: rust llvm-cov failed: {err:?}")
}

#[cfg(test)]
#[path = "rust_llvm_cov_metrics_test.rs"]
mod metrics_tests;
#[cfg(test)]
#[path = "rust_llvm_cov_test.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::build_cargo_llvm_cov_dry_run_argv;
