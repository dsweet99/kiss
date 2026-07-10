use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

#[cfg(test)]
use rust_llvm_cov_runner::{CargoLlvmCovRunRequest, build_llvm_cov_argv};
use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustLlvmCov, RustLlvmCovError, RustLlvmCovOutcome, RustLlvmCovRequest,
    cleanup_surplus_rust_cov_worker_slots, subprocess_cargo_llvm_cov_runner,
    validate_supported_rust_test_args,
};

use super::last_status::{record_statuses, rust_last_status_identity};
use super::runners::{SelectorExecutionSummary, command_stdout};

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
    assert!(jobs > 0, "jobs must be greater than zero");
    validate_supported_rust_test_args(extra)?;
    let (llvm_cov_version, rustc_version) = detect_rust_llvm_cov_versions(repo_root)?;
    let identity = rust_last_status_identity(&llvm_cov_version, &rustc_version, extra);
    let reqs: Vec<_> = selectors
        .iter()
        .map(|selector| {
            rust_llvm_cov_request_from_parts(
                repo_root,
                selector,
                extra,
                &llvm_cov_version,
                &rustc_version,
                force_rerun,
            )
        })
        .collect::<Result<_, _>>()?;
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    for result in
        run_rust_llvm_cov_requests_bounded(reqs, jobs).map_err(format_rust_llvm_cov_error)?
    {
        let outcome = result.map_err(format_rust_llvm_cov_error)?;
        print_rust_llvm_cov_outcome(&outcome);
        statuses.push((outcome.selector.clone(), outcome.status));
        summary.record(
            outcome.status,
            outcome.cache_status == RustCovCacheStatus::Hit,
            outcome.exit_code,
        );
    }
    record_statuses(repo_root, kiss::Language::Rust, &identity, &statuses)?;
    Ok(summary)
}

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

fn detect_rust_llvm_cov_versions(repo_root: &Path) -> Result<(String, String), String> {
    let cargo = PathBuf::from("cargo");
    let llvm_cov_version = command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?;
    let rustc = PathBuf::from("rustc");
    let rustc_version = command_stdout(&rustc, &["-Vv"], repo_root)?;
    Ok((llvm_cov_version, rustc_version))
}

fn run_rust_llvm_cov_requests_bounded(
    reqs: Vec<RustLlvmCovRequest>,
    jobs: usize,
) -> Result<Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>>, RustLlvmCovError> {
    run_rust_llvm_cov_requests_bounded_with_spawner(reqs, jobs, spawn_rust_llvm_cov_job)
}

type RustLlvmCovJobResult = (usize, usize, Result<RustLlvmCovOutcome, RustLlvmCovError>);

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
    let mut indexed_reqs = reqs.into_iter().enumerate();
    let mut running = 0usize;
    for slot in 0..jobs.min(len) {
        if let Some((index, mut req)) = indexed_reqs.next() {
            req.worker_slot = slot;
            spawn_job(index, slot, req, tx.clone());
            running += 1;
        }
    }

    while running > 0 {
        let Ok((index, slot, result)) = rx.recv() else {
            break;
        };
        running -= 1;
        out[index] = result;
        if let Some((next_index, mut next_req)) = indexed_reqs.next() {
            next_req.worker_slot = slot;
            spawn_job(next_index, slot, next_req, tx.clone());
            running += 1;
        }
    }
    Ok(out)
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

fn format_rust_llvm_cov_error(err: RustLlvmCovError) -> String {
    format!("error: kiss test: rust llvm-cov failed: {err:?}")
}

#[cfg(test)]
#[path = "rust_llvm_cov_test.rs"]
mod tests;
