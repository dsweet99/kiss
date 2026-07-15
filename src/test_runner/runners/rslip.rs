use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use rpytest_runner::PytestRunner;
use rslip::{CacheStatus as PyCacheStatus, Rslip, RslipError, RslipOutcome, RslipRequest};

use super::{SelectorCacheRecord, SelectorExecutionSummary, command_stdout};
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_coverage_cache_root,
};

pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    run_rslip_selectors_with_runner(
        repo_root,
        selectors,
        extra,
        force_rerun,
        jobs,
        selected_rslip_pytest_runner(),
    )
}

fn run_rslip_selectors_with_runner(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    runner: PytestRunner,
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
    let rslip = Rslip::new(runner);
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    for result in rslip.run_or_reuse_many_bounded(reqs, jobs) {
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

#[cfg(target_os = "linux")]
fn selected_rslip_pytest_runner() -> PytestRunner {
    rpytest_runner::forkserver_pytest_runner()
}

#[cfg(not(target_os = "linux"))]
fn selected_rslip_pytest_runner() -> PytestRunner {
    rpytest_runner::subprocess_pytest_runner()
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
    let repo_root = repo_root.canonicalize().map_err(|err| {
        format!(
            "error: kiss test: failed to canonicalize repository root {}: {err}",
            repo_root.display()
        )
    })?;
    Ok(RslipRequest {
        nodeid: selector.to_string(),
        cwd: repo_root.clone(),
        source_root: repo_root.clone(),
        python: PathBuf::from("python"),
        python_version: python_version.to_string(),
        pytest_version: pytest_version.to_string(),
        pytest_args: extra.to_vec(),
        env: relevant_rslip_env(PYTHON_COVERAGE_ENV_KEYS),
        cache_root: python_coverage_cache_root(&repo_root)?,
        force_rerun,
    })
}

fn relevant_rslip_env(env_keys: &[&str]) -> BTreeMap<String, String> {
    env_keys
        .iter()
        .filter_map(|key| env::var(key).ok().map(|value| ((*key).to_string(), value)))
        .collect()
}

pub(crate) fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
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
    use rpytest_runner::{PytestRunOutcome, TestStatus};
    use rslip::LineCoverage;
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::rc::Rc;
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
        let results = Rslip::new(PytestRunner::from_fn(|_| {
            panic!("empty batch should not invoke runner")
        }))
        .run_or_reuse_many_bounded(Vec::new(), 1);

        assert!(results.is_empty());
    }

    #[test]
    fn run_rslip_selectors_submits_misses_as_single_bounded_batch() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "def test_a():\n    assert True\n\n\
def test_b():\n    assert False\n",
        )
        .unwrap();
        let selectors = vec![
            "test_sample.py::test_a".to_string(),
            "test_sample.py::test_b".to_string(),
        ];
        let batch_calls = Rc::new(Cell::new(0));
        let observed_jobs = Rc::new(Cell::new(0));
        let observed_nodeids = Rc::new(RefCell::new(Vec::new()));
        let batch_calls_for_runner = Rc::clone(&batch_calls);
        let observed_jobs_for_runner = Rc::clone(&observed_jobs);
        let observed_nodeids_for_runner = Rc::clone(&observed_nodeids);
        let runner = PytestRunner::from_bounded_fn(move |reqs, jobs| {
            batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
            observed_jobs_for_runner.set(jobs);
            observed_nodeids_for_runner
                .borrow_mut()
                .extend(reqs.iter().map(|req| req.nodeid.clone()));
            reqs.into_iter()
                .map(|req| {
                    let path = req.artifacts[0].path.clone();
                    let artifact_name = req.artifacts[0].name.clone();
                    fs::write(&path, r#"{"files":{"/project/app.py":[1]}}"#).unwrap();
                    let failed = req.nodeid.ends_with("test_b");
                    Ok(PytestRunOutcome {
                        nodeid: req.nodeid,
                        status: if failed {
                            TestStatus::Failed
                        } else {
                            TestStatus::Passed
                        },
                        exit_code: Some(i32::from(failed)),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        duration: Duration::from_millis(1),
                        artifacts: BTreeMap::from([(artifact_name, path)]),
                    })
                })
                .collect()
        });

        let summary =
            run_rslip_selectors_with_runner(tmp.path(), &selectors, &[], false, 3, runner).unwrap();

        assert_eq!(batch_calls.get(), 1);
        assert_eq!(observed_jobs.get(), 3);
        assert_eq!(*observed_nodeids.borrow(), selectors);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.cache_misses, 2);
        assert_eq!(summary.cache_hits, 0);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.exit_code, 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_run_rslip_selectors_uses_isolated_forkserver_children() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("stateful.py"), "VALUE = 0\n").unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "import stateful\n\n\
def test_mutate_global():\n    stateful.VALUE = 1\n    assert stateful.VALUE == 1\n\n\
def test_global_starts_clean():\n    assert stateful.VALUE == 0\n",
        )
        .unwrap();
        let selectors = vec![
            "test_sample.py::test_mutate_global".to_string(),
            "test_sample.py::test_global_starts_clean".to_string(),
        ];

        let summary =
            run_rslip_selectors(tmp.path(), &selectors, &["-q".to_string()], true, 1).unwrap();

        assert_eq!(summary.total, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.cache_misses, 2);
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
