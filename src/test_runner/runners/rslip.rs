use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use rpytest_runner::subprocess_pytest_runner;
use rslip::{CacheStatus as PyCacheStatus, Rslip, RslipError, RslipOutcome, RslipRequest};

use super::{SelectorCacheRecord, SelectorExecutionSummary, command_stdout};
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};

pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let (python_version, pytest_version) = detect_rslip_versions(repo_root)?;
    let identity = python_last_status_identity(&python_version, &pytest_version, extra);
    let reqs: Vec<_> = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                extra,
                &python_version,
                &pytest_version,
                force_rerun,
            )
        })
        .collect::<Result<_, _>>()?;
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    for result in run_rslip_requests_bounded(reqs, jobs) {
        let outcome = result.map_err(format_rslip_error)?;
        print_rslip_outcome(&outcome);
        statuses.push((outcome.nodeid.clone(), outcome.status));
        summary.record(
            outcome.status,
            if outcome.cache_status == PyCacheStatus::Hit {
                SelectorCacheRecord::Hit
            } else {
                SelectorCacheRecord::MissStored
            },
            outcome.exit_code,
        );
    }
    record_statuses(repo_root, kiss::Language::Python, &identity, &statuses)?;
    Ok(summary)
}

pub(crate) fn rslip_request_from_parts(
    repo_root: &Path,
    selector: &str,
    extra: &[String],
    python_version: &str,
    pytest_version: &str,
    force_rerun: bool,
) -> Result<RslipRequest, String> {
    if !python_version_supports_rslip(python_version) {
        return Err(format!(
            "error: kiss test: rslip requires Python 3.12+, found {python_version}"
        ));
    }
    Ok(RslipRequest {
        nodeid: selector.to_string(),
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        python: PathBuf::from("python"),
        python_version: python_version.to_string(),
        pytest_version: pytest_version.to_string(),
        pytest_args: extra.to_vec(),
        env: BTreeMap::new(),
        cache_root: repo_root.join(".kiss").join("rslip_cache"),
        force_rerun,
    })
}

fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
    let python = PathBuf::from("python");
    let python_version = command_stdout(
        &python,
        &[
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
        repo_root,
    )?;
    let pytest_version = command_stdout(
        &python,
        &["-c", "import pytest; print(pytest.__version__)"],
        repo_root,
    )?;
    Ok((python_version, pytest_version))
}

fn python_version_supports_rslip(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 12))
}

fn run_rslip_requests_bounded(
    reqs: Vec<RslipRequest>,
    jobs: usize,
) -> Vec<Result<RslipOutcome, RslipError>> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let len = reqs.len();
    let mut out = Vec::new();
    out.resize_with(len, || {
        Err(RslipError::InvalidRequest(
            "rslip worker did not report a result".to_string(),
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
            spawn_rslip_job(index, req, tx.clone());
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
            spawn_rslip_job(next_index, next_req, tx.clone());
            running += 1;
        }
    }
    out
}

fn spawn_rslip_job(
    index: usize,
    req: RslipRequest,
    tx: mpsc::Sender<(usize, Result<RslipOutcome, RslipError>)>,
) {
    thread::spawn(move || {
        let rslip = Rslip::new(subprocess_pytest_runner());
        let result = rslip.run_or_reuse(req);
        let _ = tx.send((index, result));
    });
}

fn print_rslip_outcome(outcome: &RslipOutcome) {
    match (outcome.status, outcome.cache_status) {
        (rpytest_runner::TestStatus::Passed, PyCacheStatus::Hit) => {
            println!("PASSED (cached): {}", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Passed, PyCacheStatus::MissStored) => {
            println!("PASSED: {}", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Failed, PyCacheStatus::Hit) => {
            println!("FAILED (cached): {}", outcome.nodeid);
            eprintln!(
                "Failure output was not cached. Re-run with --force to reproduce stdout/stderr."
            );
        }
        (rpytest_runner::TestStatus::Failed, PyCacheStatus::MissStored) => {
            println!("FAILED: {}", outcome.nodeid);
            if let Some(stderr) = &outcome.stderr
                && !stderr.is_empty()
            {
                eprint!("{}", String::from_utf8_lossy(stderr));
            }
        }
    }
}

fn format_rslip_error(err: RslipError) -> String {
    format!("error: kiss test: rslip failed: {err:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rslip::LineCoverage;
    use std::time::Duration;

    #[test]
    fn format_rslip_error_includes_context() {
        let msg = format_rslip_error(RslipError::InvalidRequest("bad selector".to_string()));

        assert!(msg.contains("error: kiss test: rslip failed"));
        assert!(msg.contains("bad selector"));
    }

    #[test]
    #[should_panic(expected = "jobs must be greater than zero")]
    fn run_rslip_selectors_rejects_zero_jobs_before_spawning() {
        let tmp = tempfile::tempdir().unwrap();

        let _ = run_rslip_selectors(tmp.path(), &[], &[], false, 0);
    }

    #[test]
    fn rslip_request_and_version_contracts_are_explicit() {
        let tmp = tempfile::tempdir().unwrap();
        let extra = vec!["-q".to_string()];
        let req = rslip_request_from_parts(
            tmp.path(),
            "tests/test_app.py::test_ok",
            &extra,
            "3.12.1",
            "8.3.0",
            true,
        )
        .unwrap();

        assert_eq!(req.nodeid, "tests/test_app.py::test_ok");
        assert_eq!(req.cwd, tmp.path());
        assert_eq!(req.pytest_args, extra);
        assert!(req.force_rerun);
        assert!(python_version_supports_rslip("3.12.0"));
        assert!(python_version_supports_rslip("4.0.0"));
        assert!(!python_version_supports_rslip("3.11.9"));
    }

    #[test]
    fn bounded_rslip_runner_handles_empty_queue() {
        let results = run_rslip_requests_bounded(Vec::new(), 1);

        assert!(results.is_empty());
    }

    #[test]
    fn print_rslip_outcome_accepts_all_status_cache_shapes() {
        for (status, cache_status) in [
            (rpytest_runner::TestStatus::Passed, PyCacheStatus::Hit),
            (
                rpytest_runner::TestStatus::Passed,
                PyCacheStatus::MissStored,
            ),
            (rpytest_runner::TestStatus::Failed, PyCacheStatus::Hit),
            (
                rpytest_runner::TestStatus::Failed,
                PyCacheStatus::MissStored,
            ),
        ] {
            print_rslip_outcome(&RslipOutcome {
                nodeid: "tests/test_app.py::test_ok".to_string(),
                status,
                exit_code: Some(i32::from(status == rpytest_runner::TestStatus::Failed)),
                duration: Duration::from_millis(1),
                coverage: LineCoverage {
                    files: BTreeMap::new(),
                },
                cache_status,
                stdout: None,
                stderr: Some(Vec::new()),
            });
        }
    }
}
