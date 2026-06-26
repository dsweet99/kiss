use std::fs;
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
) -> Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>> {
    run_rust_llvm_cov_requests_bounded_with_spawner(reqs, jobs, spawn_rust_llvm_cov_job)
}

type RustLlvmCovJobResult = (usize, usize, Result<RustLlvmCovOutcome, RustLlvmCovError>);

fn run_rust_llvm_cov_requests_bounded_with_spawner<F>(
    reqs: Vec<RustLlvmCovRequest>,
    jobs: usize,
    mut spawn_job: F,
) -> Vec<Result<RustLlvmCovOutcome, RustLlvmCovError>>
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
        return out;
    }

    cleanup_surplus_worker_slots(&reqs, jobs);
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
    out
}

fn cleanup_surplus_worker_slots(reqs: &[RustLlvmCovRequest], jobs: usize) {
    let mut cache_roots = Vec::new();
    for req in reqs {
        if cache_roots.iter().any(|root| root == &req.cache_root) {
            continue;
        }
        cache_roots.push(req.cache_root.clone());
    }
    for cache_root in cache_roots {
        let workers_root = cache_root.join("workers");
        let Ok(entries) = fs::read_dir(workers_root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(slot_text) = name.strip_prefix("slot-") else {
                continue;
            };
            let should_remove = slot_text.parse::<usize>().map_or(true, |slot| slot >= jobs);
            if should_remove {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
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
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use rust_llvm_cov_runner::RustLineCoverage;

    #[test]
    fn format_rust_llvm_cov_error_preserves_context_and_message() {
        let msg = format_rust_llvm_cov_error(RustLlvmCovError::InvalidRequest(
            "bad selector".to_string(),
        ));

        assert!(msg.contains("rust llvm-cov failed"));
        assert!(msg.contains("bad selector"));
    }

    #[test]
    fn bounded_runner_assigns_and_reuses_worker_slots() {
        let tmp = tempfile::tempdir().unwrap();
        let reqs: Vec<_> = (0..5)
            .map(|index| RustLlvmCovRequest {
                selector: format!("tests::case_{index}"),
                cwd: tmp.path().to_path_buf(),
                source_root: tmp.path().to_path_buf(),
                cargo: PathBuf::from("cargo"),
                llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
                rustc_version: "rustc 1.88.0".to_string(),
                cargo_args: Vec::new(),
                test_args: Vec::new(),
                env: BTreeMap::new(),
                cache_root: tmp.path().join(".kiss").join("rust_llvm_cov_cache"),
                force_rerun: false,
                worker_slot: usize::MAX,
            })
            .collect();
        let seen_slots = Rc::new(RefCell::new(Vec::new()));
        let seen_slots_for_spawner = Rc::clone(&seen_slots);

        let results = run_rust_llvm_cov_requests_bounded_with_spawner(
            reqs,
            2,
            move |index, slot, req, tx| {
                assert_eq!(req.worker_slot, slot);
                seen_slots_for_spawner.borrow_mut().push(slot);
                let outcome = RustLlvmCovOutcome {
                    selector: req.selector,
                    status: rpytest_runner::TestStatus::Passed,
                    exit_code: Some(0),
                    duration: Duration::from_millis(1),
                    coverage: RustLineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status: RustCovCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                };
                tx.send((index, slot, Ok(outcome))).unwrap();
            },
        );

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(&*seen_slots.borrow(), &[0, 1, 0, 1, 0]);
    }

    #[test]
    fn bounded_runner_removes_surplus_worker_slots() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
        for slot in 0..3 {
            fs::create_dir_all(
                cache_root
                    .join("workers")
                    .join(format!("slot-{slot}"))
                    .join("target"),
            )
            .unwrap();
        }
        let reqs: Vec<_> = (0..2)
            .map(|index| RustLlvmCovRequest {
                selector: format!("tests::case_{index}"),
                cwd: tmp.path().to_path_buf(),
                source_root: tmp.path().to_path_buf(),
                cargo: PathBuf::from("cargo"),
                llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
                rustc_version: "rustc 1.88.0".to_string(),
                cargo_args: Vec::new(),
                test_args: Vec::new(),
                env: BTreeMap::new(),
                cache_root: cache_root.clone(),
                force_rerun: false,
                worker_slot: usize::MAX,
            })
            .collect();

        let results = run_rust_llvm_cov_requests_bounded_with_spawner(
            reqs,
            2,
            move |index, slot, req, tx| {
                let outcome = RustLlvmCovOutcome {
                    selector: req.selector,
                    status: rpytest_runner::TestStatus::Passed,
                    exit_code: Some(0),
                    duration: Duration::from_millis(1),
                    coverage: RustLineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status: RustCovCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                };
                tx.send((index, slot, Ok(outcome))).unwrap();
            },
        );

        assert!(results.iter().all(Result::is_ok));
        assert!(cache_root.join("workers").join("slot-0").exists());
        assert!(cache_root.join("workers").join("slot-1").exists());
        assert!(!cache_root.join("workers").join("slot-2").exists());
    }

    #[test]
    fn spawn_rust_llvm_cov_job_reports_runner_errors_with_index_and_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let req = RustLlvmCovRequest {
            selector: "tests::case".to_string(),
            cwd: tmp.path().to_path_buf(),
            source_root: tmp.path().to_path_buf(),
            cargo: PathBuf::from("/definitely/not/cargo"),
            llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
            rustc_version: "rustc 1.88.0".to_string(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
            cache_root: tmp.path().join(".kiss").join("rust_llvm_cov_cache"),
            force_rerun: true,
            worker_slot: 4,
        };
        let (tx, rx) = mpsc::channel();

        spawn_rust_llvm_cov_job(7, 4, req, tx);
        let (index, slot, result) = rx.recv_timeout(Duration::from_secs(2)).unwrap();

        assert_eq!(index, 7);
        assert_eq!(slot, 4);
        assert!(matches!(result, Err(RustLlvmCovError::Runner(_))));
    }
}
