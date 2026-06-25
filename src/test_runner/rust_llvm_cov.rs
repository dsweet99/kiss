use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use rust_llvm_cov_runner::{
    CargoLlvmCovRunRequest, RustCovCacheStatus, RustLlvmCov, RustLlvmCovError, RustLlvmCovOutcome,
    RustLlvmCovRequest, build_llvm_cov_argv, subprocess_cargo_llvm_cov_runner,
};

use super::runners::{command_stdout, merge_exit_codes};

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
) -> Result<i32, String> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let (llvm_cov_version, rustc_version) = detect_rust_llvm_cov_versions(repo_root)?;
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
    let mut code = 0;
    for result in run_rust_llvm_cov_requests_bounded(reqs, jobs) {
        let outcome = result.map_err(format_rust_llvm_cov_error)?;
        print_rust_llvm_cov_outcome(&outcome);
        if outcome.status == rpytest_runner::TestStatus::Failed {
            code = merge_exit_codes(code, outcome.exit_code.unwrap_or(1));
        }
    }
    Ok(code)
}

pub(crate) fn rust_llvm_cov_request_from_parts(
    repo_root: &Path,
    selector: &str,
    extra: &[String],
    llvm_cov_version: &str,
    rustc_version: &str,
    force_rerun: bool,
) -> Result<RustLlvmCovRequest, String> {
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
) -> Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let len = reqs.len();
    let mut out = Vec::new();
    out.resize_with(len, || {
        Err(RustLlvmCovError::InvalidRequest(
            "rust llvm-cov worker did not report a result".to_string(),
        ))
    });
    if len == 0 {
        return out;
    }

    let (tx, rx) = mpsc::channel();
    let mut indexed_reqs = reqs.into_iter().enumerate();
    let mut running = 0usize;
    for _ in 0..jobs.min(len) {
        if let Some((index, req)) = indexed_reqs.next() {
            spawn_rust_llvm_cov_job(index, req, tx.clone());
            running += 1;
        }
    }

    while running > 0 {
        let Ok((index, result)) = rx.recv() else {
            break;
        };
        running -= 1;
        out[index] = result;
        if let Some((next_index, next_req)) = indexed_reqs.next() {
            spawn_rust_llvm_cov_job(next_index, next_req, tx.clone());
            running += 1;
        }
    }
    out
}

fn spawn_rust_llvm_cov_job(
    index: usize,
    req: RustLlvmCovRequest,
    tx: mpsc::Sender<(usize, Result<RustLlvmCovOutcome, RustLlvmCovError>)>,
) {
    thread::spawn(move || {
        let runner = RustLlvmCov::new(subprocess_cargo_llvm_cov_runner());
        let result = runner.run_or_reuse(req);
        let _ = tx.send((index, result));
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
    }
}

fn format_rust_llvm_cov_error(err: RustLlvmCovError) -> String {
    format!("error: kiss test: rust llvm-cov failed: {err:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rust_llvm_cov_error_preserves_context_and_message() {
        let msg = format_rust_llvm_cov_error(RustLlvmCovError::InvalidRequest(
            "bad selector".to_string(),
        ));

        assert!(msg.contains("rust llvm-cov failed"));
        assert!(msg.contains("bad selector"));
    }
}
