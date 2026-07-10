use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use rust_llvm_cov_runner::{CargoLlvmCovRunRequest, build_llvm_cov_argv};
use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchRequest, RustCoverageBatchResult, RustLlvmCov,
    RustLlvmCovError, RustLlvmCovOutcome, RustLlvmCovRequest, build_rust_coverage_batch_plan,
    cleanup_surplus_rust_cov_worker_slots, subprocess_cargo_llvm_cov_runner,
    validate_supported_rust_test_args,
};

use super::last_status::{LastStatusIdentity, record_statuses, rust_last_status_identity};
use super::runners::{SelectorCacheRecord, SelectorExecutionSummary, command_stdout};

#[cfg(test)]
pub(crate) fn build_cargo_llvm_cov_dry_run_argv(selector: &str, extra: &[String]) -> Vec<String> {
    let mut req = CargoLlvmCovRunRequest::new(
        selector,
        PathBuf::from("."),
        PathBuf::from("cargo"),
        PathBuf::from("<coverage.json>"),
    );
    req.test_args = extra.to_vec();
    build_llvm_cov_argv(&req)
}

pub(crate) fn run_rust_llvm_cov_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        extra,
        force_rerun,
        jobs,
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

fn run_rust_llvm_cov_selectors_with_deps<D, E>(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
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
    let batch_req =
        rust_coverage_batch_request_from_parts(repo_root, selectors, extra, force_rerun, jobs)?;
    build_rust_coverage_batch_plan(&batch_req)?;
    let versions = detect_versions(repo_root)?;
    let identity = rust_last_status_identity(
        &versions.cargo,
        &versions.llvm_cov,
        &versions.rustc,
        &versions.cargo_nextest,
        extra,
    );
    let result = execute_batch(&batch_req, &versions)?;
    finish_rust_coverage_batch_result(repo_root, &identity, result)
}

fn execute_rust_coverage_batch_compat(
    batch_req: &RustCoverageBatchRequest,
    versions: &RustCoverageToolVersions,
) -> Result<RustCoverageBatchResult, String> {
    let reqs: Vec<_> = batch_req
        .logical_selectors
        .iter()
        .map(|selector| {
            rust_llvm_cov_request_from_batch_parts(
                batch_req,
                selector,
                &versions.llvm_cov,
                &versions.rustc,
            )
        })
        .collect::<Result<_, _>>()?;
    let mut completed = Vec::new();
    let mut batch_error = None;
    for result in run_rust_llvm_cov_requests_bounded(reqs, batch_req.jobs)
        .map_err(format_rust_llvm_cov_error)?
    {
        match result {
            Ok(outcome) => completed.push(outcome),
            Err(err) if batch_error.is_none() => batch_error = Some(err),
            Err(_) => {}
        }
    }
    Ok(RustCoverageBatchResult {
        completed,
        batch_error,
        counters: Default::default(),
    })
}

#[cfg(test)]
pub(crate) fn rust_llvm_cov_request_from_parts(
    repo_root: &Path,
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

fn rust_llvm_cov_request_from_batch_parts(
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

pub(crate) fn rust_coverage_batch_request_from_parts(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
) -> Result<RustCoverageBatchRequest, String> {
    validate_supported_rust_test_args(extra)?;
    Ok(RustCoverageBatchRequest {
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo_root.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: Vec::new(),
        test_args: extra.to_vec(),
        env: BTreeMap::new(),
        force_rerun,
        jobs,
        generated_config: unique_rust_coverage_batch_config_path(repo_root),
    })
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

fn run_rust_llvm_cov_requests_bounded(
    reqs: Vec<RustLlvmCovRequest>,
    jobs: usize,
) -> Result<Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>>, RustLlvmCovError> {
    run_rust_llvm_cov_requests_bounded_with_spawner(reqs, jobs, spawn_rust_llvm_cov_job)
}

type RustLlvmCovJobResult = (usize, usize, Result<RustLlvmCovOutcome, RustLlvmCovError>);
type RustLlvmCovIndexedRequests = std::iter::Enumerate<std::vec::IntoIter<RustLlvmCovRequest>>;

fn run_rust_llvm_cov_requests_bounded_with_spawner<F>(
    reqs: Vec<RustLlvmCovRequest>,
    jobs: usize,
    mut spawn_job: F,
) -> Result<Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>>, RustLlvmCovError>
where
    F: FnMut(usize, usize, RustLlvmCovRequest, mpsc::Sender<RustLlvmCovJobResult>),
{
    assert!(jobs > 0, "jobs must be greater than zero");
    let len = reqs.len();
    let mut out = Vec::new();
    out.resize_with(len, || {
        Err(RustLlvmCovError::InvalidRequest(
            "rust llvm-cov worker did not report a result".to_string(),
        ))
    });
    if len == 0 {
        return Ok(out);
    }

    cleanup_surplus_worker_slots(&reqs, jobs)?;
    let (tx, rx) = mpsc::channel();
    let mut parent_tx = Some(tx);
    let mut indexed_reqs = reqs.into_iter().enumerate();
    let mut running = 0usize;
    for slot in 0..jobs.min(len) {
        if spawn_next_rust_llvm_cov_job(&mut indexed_reqs, slot, &mut parent_tx, &mut spawn_job) {
            running += 1;
        }
    }

    while running > 0 {
        let Ok((index, slot, result)) = rx.recv() else {
            break;
        };
        running -= 1;
        out[index] = result;
        if spawn_next_rust_llvm_cov_job(&mut indexed_reqs, slot, &mut parent_tx, &mut spawn_job) {
            running += 1;
        }
    }
    Ok(out)
}

fn spawn_next_rust_llvm_cov_job<F>(
    indexed_reqs: &mut RustLlvmCovIndexedRequests,
    slot: usize,
    parent_tx: &mut Option<mpsc::Sender<RustLlvmCovJobResult>>,
    spawn_job: &mut F,
) -> bool
where
    F: FnMut(usize, usize, RustLlvmCovRequest, mpsc::Sender<RustLlvmCovJobResult>),
{
    let Some((index, mut req)) = indexed_reqs.next() else {
        return false;
    };
    req.worker_slot = slot;
    let tx = parent_tx.as_ref().expect("sender is live while spawning");
    spawn_job(index, slot, req, tx.clone());
    if indexed_reqs.len() == 0 {
        drop(parent_tx.take());
    }
    true
}

fn cleanup_surplus_worker_slots(
    reqs: &[RustLlvmCovRequest],
    jobs: usize,
) -> Result<(), RustLlvmCovError> {
    let mut cache_roots = Vec::new();
    for req in reqs {
        if cache_roots.iter().any(|root| root == &req.cache_root) {
            continue;
        }
        cache_roots.push(req.cache_root.clone());
    }
    for cache_root in cache_roots {
        cleanup_surplus_rust_cov_worker_slots(&cache_root, jobs)?;
    }
    Ok(())
}

fn spawn_rust_llvm_cov_job(
    index: usize,
    slot: usize,
    req: RustLlvmCovRequest,
    tx: mpsc::Sender<RustLlvmCovJobResult>,
) {
    thread::spawn(move || {
        let runner = RustLlvmCov::new(subprocess_cargo_llvm_cov_runner());
        let result = runner.run_or_reuse(req);
        let _ = tx.send((index, slot, result));
    });
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
fn passed_rust_llvm_cov_outcome(selector: String) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector,
        status: rpytest_runner::TestStatus::Passed,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
        coverage: rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::new(),
        },
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

#[cfg(test)]
#[path = "rust_llvm_cov_test.rs"]
mod tests;

#[cfg(test)]
#[path = "rust_llvm_cov_bounded_test.rs"]
mod bounded_tests;
