use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use rpytest_runner::PytestRunner;
use rslip::{
    CacheStatus as PyCacheStatus, Rslip, RslipBatchProgress, RslipError, RslipOutcome, RslipRequest,
};
use std::time::Duration;

use super::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary, command_stdout,
};
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};
use crate::test_runner::python_coverage_index::python_coverage_cache_root;

pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
    content_fingerprint: Option<String>,
) -> Result<SelectorExecutionSummary, String> {
    run_rslip_selectors_with_runner(
        RslipBatchArgs {
            repo_root,
            selectors,
            extra,
            force_rerun,
            force_rerun_selectors,
            jobs,
            content_fingerprint,
        },
        selected_rslip_pytest_runner(),
    )
}

/// Default per-test ceiling for python population/selective runs.
/// Large enough for sameq slow tests (~137s observed), short enough to stop
/// hung webtester/network tests from blocking `kiss test .` for hours.
pub(crate) const DEFAULT_PYTEST_TIMEOUT: Duration = Duration::from_secs(180);

struct RslipBatchArgs<'a> {
    repo_root: &'a Path,
    selectors: &'a [String],
    extra: &'a [String],
    force_rerun: bool,
    force_rerun_selectors: &'a [String],
    jobs: usize,
    content_fingerprint: Option<String>,
}

fn run_rslip_selectors_with_runner(
    args: RslipBatchArgs<'_>,
    runner: PytestRunner,
) -> Result<SelectorExecutionSummary, String> {
    assert!(args.jobs > 0, "jobs must be greater than zero");
    let (python_version, pytest_version) = detect_rslip_versions(args.repo_root)?;
    let identity = python_last_status_identity(&python_version, &pytest_version, args.extra);
    let force_set: BTreeSet<&str> = args
        .force_rerun_selectors
        .iter()
        .map(|selector| selector.as_str())
        .collect();
    // Load gate once for the batch so concurrent `.kissconfig` writers cannot
    // change kill timeouts mid-population.
    let gate = kiss::GateConfig::load();
    // Build shared request fields once; only nodeid / force_rerun / timeout vary.
    let mut template = rslip_request_from_parts(
        args.repo_root,
        "",
        args.extra,
        &python_version,
        &pytest_version,
        false,
    )?;
    template.content_fingerprint = args.content_fingerprint;
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    let mut stdout = std::io::BufWriter::new(std::io::stdout());
    let (reqs, runnable_selectors) = partition_rslip_requests(
        PartitionInput {
            selectors: args.selectors,
            template: &template,
            force_rerun: args.force_rerun,
            force_set: &force_set,
            gate: &gate,
        },
        &mut summary,
        &mut statuses,
        &mut stdout,
    );
    let results = Rslip::new(runner).run_or_reuse_many_bounded_with_progress(
        reqs,
        args.jobs,
        |event| {
            handle_rslip_batch_progress(event, &runnable_selectors, &mut stdout);
        },
    );
    let _ = std::io::Write::flush(&mut stdout);
    for (selector, result) in runnable_selectors.iter().zip(results) {
        record_rslip_selector_result(selector, result, &gate, &mut summary, &mut statuses);
    }
    record_statuses(args.repo_root, kiss::Language::Python, &identity, &statuses)?;
    Ok(summary)
}

struct PartitionInput<'a> {
    selectors: &'a [String],
    template: &'a RslipRequest,
    force_rerun: bool,
    force_set: &'a BTreeSet<&'a str>,
    gate: &'a kiss::GateConfig,
}

fn partition_rslip_requests(
    input: PartitionInput<'_>,
    summary: &mut SelectorExecutionSummary,
    statuses: &mut Vec<(String, rpytest_runner::TestStatus)>,
    stdout: &mut impl Write,
) -> (Vec<RslipRequest>, Vec<String>) {
    let mut reqs = Vec::new();
    let mut runnable = Vec::new();
    for selector in input.selectors {
        let timeout = timeout_for_selector_with_gate(input.gate, selector);
        // Ban path (limit <= 0): do not invoke the runner; mark TIMEOUT immediately.
        if timeout.is_zero() {
            record_immediate_timeout(selector, summary, statuses, stdout);
            continue;
        }
        let mut req = input.template.clone();
        req.nodeid = selector.clone();
        // Template uses selector "" for shared fields; per-selector timeout
        // must follow path-pattern limits (not the catch-all from "").
        req.timeout = Some(timeout);
        req.force_rerun = input.force_rerun || input.force_set.contains(selector.as_str());
        reqs.push(req);
        runnable.push(selector.clone());
    }
    (reqs, runnable)
}

fn record_immediate_timeout(
    selector: &str,
    summary: &mut SelectorExecutionSummary,
    statuses: &mut Vec<(String, rpytest_runner::TestStatus)>,
    stdout: &mut impl Write,
) {
    let status = rpytest_runner::TestStatus::TimedOut;
    let line = crate::test_runner::status_labels::format_status_line(status, selector, "", None);
    let _ = writeln!(stdout, "{line}");
    statuses.push((selector.to_string(), status));
    summary.record(SelectorExecutionRecord {
        selector: selector.to_string(),
        status,
        cache_record: SelectorCacheRecord::MissUnstored,
        exit_code: Some(124),
        duration: Duration::ZERO,
    });
}

fn record_rslip_selector_result(
    selector: &str,
    result: Result<RslipOutcome, RslipError>,
    gate: &kiss::GateConfig,
    summary: &mut SelectorExecutionSummary,
    statuses: &mut Vec<(String, rpytest_runner::TestStatus)>,
) {
    match result {
        Ok(outcome) => {
            let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
                outcome.status,
                &outcome.nodeid,
                outcome.duration,
                gate,
            );
            statuses.push((outcome.nodeid.clone(), status));
            summary.record(SelectorExecutionRecord {
                selector: outcome.nodeid.clone(),
                status,
                cache_record: if outcome.cache_status == PyCacheStatus::Hit {
                    SelectorCacheRecord::Hit
                } else {
                    SelectorCacheRecord::MissStored
                },
                exit_code: outcome.exit_code,
                duration: outcome.duration,
            });
        }
        // Keep population/selective batches moving: one rslip Io/runner error must
        // not discard thousands of already-resolved cache hits.
        Err(_) => {
            statuses.push((selector.to_string(), rpytest_runner::TestStatus::Failed));
            summary.record(SelectorExecutionRecord {
                selector: selector.to_string(),
                status: rpytest_runner::TestStatus::Failed,
                cache_record: SelectorCacheRecord::MissUnstored,
                exit_code: Some(1),
                duration: Duration::ZERO,
            });
        }
    }
}

fn handle_rslip_batch_progress(
    event: RslipBatchProgress,
    selectors: &[String],
    stdout: &mut impl Write,
) {
    match event {
        RslipBatchProgress::Prepared {
            cache_hits,
            cache_misses,
        } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: rslip prepared hits={cache_hits} misses={cache_misses}"
            ));
        }
        RslipBatchProgress::SelectorFinalized { outcomes } => {
            for (index, result) in outcomes {
                match result {
                    Ok(outcome) => {
                        print_rslip_outcome(&outcome, stdout);
                    }
                    Err(err) => {
                        let selector = selectors
                            .get(index)
                            .map(String::as_str)
                            .unwrap_or("<unknown>");
                        let _ = writeln!(stdout, "FAIL: {selector} (rslip error)");
                        eprintln!("{}", format_rslip_error(err));
                    }
                }
            }
            let _ = stdout.flush();
        }
        RslipBatchProgress::CachedStatusDump { body } => {
            let _ = stdout.write_all(body.as_bytes());
            let _ = stdout.flush();
        }
        RslipBatchProgress::TestsRemaining { remaining } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: tests_remaining={remaining}"
            ));
        }
    }
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
        timeout: Some(timeout_for_selector(selector)),
        content_fingerprint: None,
    })
}

fn timeout_for_selector(selector: &str) -> Duration {
    timeout_for_selector_with_gate(&kiss::GateConfig::load(), selector)
}

fn timeout_for_selector_with_gate(gate: &kiss::GateConfig, selector: &str) -> Duration {
    if gate.unit_test_time_gate_disabled() {
        return DEFAULT_PYTEST_TIMEOUT;
    }
    let limit = gate.unit_test_seconds_limit(selector);
    if limit <= 0.0 {
        // Ban path: zero/negative limit ⇒ caller short-circuits to TIMEOUT
        // without invoking the pytest runner.
        return Duration::ZERO;
    }
    let limit = Duration::from_secs_f64(limit);
    if limit < DEFAULT_PYTEST_TIMEOUT {
        limit
    } else {
        DEFAULT_PYTEST_TIMEOUT
    }
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

fn print_rslip_outcome(outcome: &RslipOutcome, out: &mut impl std::io::Write) {
    let gate = kiss::GateConfig::load();
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        &outcome.nodeid,
        outcome.duration,
        &gate,
    );
    let duration = crate::test_runner::duration::format_test_duration(outcome.duration);
    let cache_tag = match outcome.cache_status {
        PyCacheStatus::Hit => Some("cached"),
        PyCacheStatus::MissStored => None,
    };
    let line = crate::test_runner::status_labels::format_status_line(
        status,
        &outcome.nodeid,
        if cache_tag.is_some() { "" } else { &duration },
        cache_tag,
    );
    let _ = writeln!(out, "{line}");
    if matches!(
        status,
        rpytest_runner::TestStatus::Failed | rpytest_runner::TestStatus::TimedOut
    ) && outcome.cache_status != PyCacheStatus::Hit
        && let Some(stderr) = &outcome.stderr
        && !stderr.is_empty()
    {
        eprint!("{}", String::from_utf8_lossy(stderr));
    }
}

fn format_rslip_error(err: RslipError) -> String {
    format!("error: kiss test: rslip failed: {err:?}")
}

#[cfg(test)]
#[path = "rslip_test.rs"]
mod tests;
