use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rpytest_runner::PytestRunner;
use rslip::{
    CacheStatus as PyCacheStatus, Rslip, RslipBatchProgress, RslipError, RslipOutcome, RslipRequest,
};
use std::time::Duration;

use super::{SelectorCacheRecord, SelectorExecutionSummary, command_stdout};
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};
use crate::test_runner::python_coverage_index::python_coverage_cache_root;

pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    run_rslip_selectors_with_runner(
        repo_root,
        selectors,
        extra,
        force_rerun,
        force_rerun_selectors,
        jobs,
        selected_rslip_pytest_runner(),
    )
}

/// Default per-test ceiling for python population/selective runs.
/// Large enough for sameq slow tests (~137s observed), short enough to stop
/// hung webtester/network tests from blocking `kiss test .` for hours.
pub(crate) const DEFAULT_PYTEST_TIMEOUT: Duration = Duration::from_secs(180);

fn run_rslip_selectors_with_runner(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
    runner: PytestRunner,
) -> Result<SelectorExecutionSummary, String> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let (python_version, pytest_version) = detect_rslip_versions(repo_root)?;
    let identity = python_last_status_identity(&python_version, &pytest_version, extra);
    let force_set: BTreeSet<&str> = force_rerun_selectors
        .iter()
        .map(|selector| selector.as_str())
        .collect();
    let reqs: Vec<_> = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                extra,
                &python_version,
                &pytest_version,
                force_rerun || force_set.contains(selector.as_str()),
            )
        })
        .collect::<Result<_, _>>()?;
    let rslip = Rslip::new(runner);
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    let results = rslip.run_or_reuse_many_bounded_with_progress(reqs, jobs, |event| match event {
        RslipBatchProgress::Prepared {
            cache_hits,
            cache_misses,
        } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: rslip prepared hits={cache_hits} misses={cache_misses}"
            ));
        }
        RslipBatchProgress::Resolved { remaining_misses } => {
            // Periodic heartbeat so large silent miss batches do not look hung.
            if remaining_misses == 0 || remaining_misses % 25 == 0 {
                crate::test_runner::emit_test_progress(&format!(
                    "kiss test: rslip misses remaining={remaining_misses}"
                ));
            }
        }
    });
    for (selector, result) in selectors.iter().zip(results) {
        match result {
            Ok(outcome) => {
                print_rslip_outcome(&outcome);
                let _ = std::io::Write::flush(&mut std::io::stdout());
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
            // Keep population/selective batches moving: one rslip Io/runner error must
            // not discard thousands of already-resolved cache hits.
            Err(err) => {
                println!("FAILED: {selector} (rslip error)");
                eprintln!("{}", format_rslip_error(err));
                let _ = std::io::Write::flush(&mut std::io::stdout());
                statuses.push((selector.clone(), rpytest_runner::TestStatus::Failed));
                summary.record(
                    rpytest_runner::TestStatus::Failed,
                    SelectorCacheRecord::MissUnstored,
                    Some(1),
                );
            }
        }
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
        env: kiss::python_coverage_env_map(&repo_root),
        cache_root: python_coverage_cache_root(&repo_root)?,
        force_rerun,
        timeout: Some(DEFAULT_PYTEST_TIMEOUT),
    })
}

pub(crate) fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
    if let Some(cached) = read_cached_python_tool_versions(repo_root) {
        return Ok(cached);
    }
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
    let _ = write_cached_python_tool_versions(repo_root, &python_version, &pytest_version);
    Ok((python_version, pytest_version))
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PythonToolVersionsCache {
    python: String,
    pytest: String,
}

fn python_tool_versions_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("python_tool_versions.json")
}

fn read_cached_python_tool_versions(repo_root: &Path) -> Option<(String, String)> {
    let bytes = std::fs::read(python_tool_versions_cache_path(repo_root)).ok()?;
    let cached: PythonToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    Some((cached.python, cached.pytest))
}

fn write_cached_python_tool_versions(
    repo_root: &Path,
    python: &str,
    pytest: &str,
) -> std::io::Result<()> {
    let path = python_tool_versions_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = PythonToolVersionsCache {
        python: python.to_string(),
        pytest: pytest.to_string(),
    };
    let bytes = serde_json::to_vec(&cached).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

fn python_version_supports_rslip(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 12))
}

fn print_rslip_outcome(outcome: &RslipOutcome) {
    let duration = crate::test_runner::duration::format_test_duration(outcome.duration);
    match (outcome.status, outcome.cache_status) {
        (rpytest_runner::TestStatus::Passed, PyCacheStatus::Hit) => {
            println!("PASSED (cached): {}", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Passed, PyCacheStatus::MissStored) => {
            println!("PASSED: {} ({duration})", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Failed, PyCacheStatus::Hit) => {
            println!("FAILED (cached): {}", outcome.nodeid);
            eprintln!(
                "Failure output was not cached. Re-run with --force to reproduce stdout/stderr."
            );
        }
        (rpytest_runner::TestStatus::Failed, PyCacheStatus::MissStored) => {
            println!("FAILED: {} ({duration})", outcome.nodeid);
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
#[path = "rslip_test.rs"]
mod tests;
