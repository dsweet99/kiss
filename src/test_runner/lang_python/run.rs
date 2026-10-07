//! Runs Python tests with pytest, one node id per request, and stores each result.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner, TestStatus,
};

use super::records::RecordWriter;
use super::versions::{pytest_env, timeout_for_selector_with_gate};
use crate::test_runner::runners::{SelectorCacheRecord, SelectorExecutionRecord};

pub(crate) struct PytestSelectorsArgs<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) selectors: &'a [String],
    pub(crate) extra: &'a [String],
    pub(crate) force_rerun: bool,
    pub(crate) jobs: usize,
    pub(crate) gate: &'a kiss::GateConfig,
}

fn clamp_pytest_jobs(requested: usize) -> usize {
    let cfg = kiss::TestSectionConfig::load();
    let mut requested = requested.max(1);
    let Some(cap) = cfg.num_jobs_pytest_explicit else {
        return requested;
    };
    let cap = cap.max(1);
    // Omitted `-j` arrives as `num_jobs`. An explicit pytest count is the
    // worker budget then. A different `-j` is left alone, then capped.
    if requested == cfg.num_jobs.max(1) && cap > requested {
        requested = cap;
    }
    requested.clamp(1, cap)
}

pub(crate) fn run_pytest_selectors(
    args: &PytestSelectorsArgs<'_>,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<(), String> {
    run_pytest_selectors_with_runner(args, &selected_pytest_runner(), on_result)
}

pub(super) fn run_pytest_selectors_with_runner(
    args: &PytestSelectorsArgs<'_>,
    runner: &PytestRunner,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<(), String> {
    assert!(args.jobs > 0, "jobs must be greater than zero");
    let jobs = clamp_pytest_jobs(args.jobs);
    if jobs < args.jobs {
        crate::test_runner::emit_test_progress(&format!(
            "kiss test: pytest workers={jobs} (capped from {})",
            args.jobs
        ));
    }
    let repo_root = args.repo_root.canonicalize().map_err(|err| {
        format!(
            "error: kiss test: failed to canonicalize repository root {}: {err}",
            args.repo_root.display()
        )
    })?;
    let identity = super::records::record_identity(&repo_root, args.extra)?;
    let writer = RecordWriter::new(&repo_root, identity)?;
    let mut runnable = Vec::new();
    for selector in args.selectors {
        if timeout_for_selector_with_gate(args.gate, selector).is_zero() {
            on_result(immediate_timeout_record(selector));
        } else {
            runnable.push(selector.clone());
        }
    }
    if runnable.is_empty() {
        return Ok(());
    }
    purge_stale_bytecode(&repo_root, &runnable, args.force_rerun);
    let reqs: Vec<PytestRunRequest> = runnable
        .iter()
        .map(|selector| pytest_request(&repo_root, selector, args))
        .collect();
    let mut remaining = runnable.len();
    set_python_remaining(remaining);
    let mut store_error = None;
    runner.run_many_bounded_with_on_complete(reqs, jobs, &mut |index, result| {
        let selector = &runnable[index];
        let finished = finished_test(selector, result, args.gate);
        // Publish the record before the PASS/FAIL line. Callers treat that line as
        // proof the result is durable; `kiss_test_sigint_caches_passed_tests_as_it_goes`
        // sends SIGINT as soon as it sees the line.
        let stored = writer.store(selector, finished.status, finished.duration, args.gate);
        print_finished_test(&finished, args.gate);
        let cache_record = if stored.is_ok() {
            SelectorCacheRecord::MissStored
        } else {
            SelectorCacheRecord::MissUnstored
        };
        if let Err(err) = stored {
            store_error.get_or_insert(err);
        }
        on_result(selector_record(
            selector,
            &finished,
            cache_record,
            args.gate,
        ));
        remaining = remaining.saturating_sub(1);
        set_python_remaining(remaining);
    });
    match store_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn set_python_remaining(remaining: usize) {
    crate::test_runner::tests_remaining::set_language_remaining(kiss::Language::Python, remaining);
}

fn purge_stale_bytecode(repo_root: &Path, selectors: &[String], force_rerun: bool) {
    if force_rerun {
        super::pycache::purge_pycache_under(repo_root);
        return;
    }
    let modules: BTreeSet<&str> = selectors
        .iter()
        .map(|selector| selector.split("::").next().unwrap_or(selector))
        .collect();
    for module in modules {
        super::pycache::purge_pyc_for_nodeid(repo_root, module);
    }
}

fn pytest_request(
    repo_root: &Path,
    selector: &str,
    args: &PytestSelectorsArgs<'_>,
) -> PytestRunRequest {
    let cache_dir = crate::test_runner::test_state_dir(repo_root).join("pytest_cache");
    let mut pytest_args = vec![
        "-o".to_string(),
        format!("cache_dir={}", cache_dir.to_string_lossy()),
    ];
    pytest_args.extend(args.extra.iter().cloned());
    PytestRunRequest::from_parts(
        selector.to_string(),
        repo_root.to_path_buf(),
        std::path::PathBuf::from("python"),
        pytest_args,
        pytest_env(repo_root),
        Vec::new(),
        Vec::new(),
        Some(timeout_for_selector_with_gate(args.gate, selector)),
    )
}

/// The result of one pytest node id, before the time gate is applied.
pub(super) struct FinishedTest {
    pub(super) nodeid: String,
    pub(super) status: TestStatus,
    pub(super) exit_code: i32,
    pub(super) duration: Duration,
    pub(super) stderr: Vec<u8>,
}

pub(super) fn finished_test(
    selector: &str,
    result: Result<PytestRunOutcome, PytestRunError>,
    gate: &kiss::GateConfig,
) -> FinishedTest {
    match result {
        Ok(outcome) => FinishedTest {
            nodeid: selector.to_string(),
            status: outcome.status,
            exit_code: outcome
                .exit_code
                .unwrap_or(i32::from(outcome.status != TestStatus::Passed)),
            duration: outcome.duration,
            stderr: outcome.stderr,
        },
        Err(err) if runner_error_is_timeout(&err) => FinishedTest {
            nodeid: selector.to_string(),
            status: TestStatus::TimedOut,
            exit_code: 124,
            duration: match err {
                PytestRunError::Timeout(timeout) => timeout,
                _ => timeout_for_selector_with_gate(gate, selector),
            },
            stderr: Vec::new(),
        },
        Err(err) => FinishedTest {
            nodeid: selector.to_string(),
            status: TestStatus::Failed,
            exit_code: 1,
            duration: Duration::ZERO,
            stderr: format!("error: kiss test: pytest runner failed: {err:?}\n").into_bytes(),
        },
    }
}

pub(super) fn runner_error_is_timeout(err: &PytestRunError) -> bool {
    match err {
        PytestRunError::Timeout(_) => true,
        PytestRunError::Protocol(message) => {
            message.contains("module batch result missing")
                || message.contains("module batch timed out")
        }
        _ => false,
    }
}

fn gated_status(finished: &FinishedTest, gate: &kiss::GateConfig) -> TestStatus {
    crate::test_runner::status_labels::apply_unit_test_time_limit(
        finished.status,
        &finished.nodeid,
        finished.duration,
        gate,
    )
}

pub(super) fn print_finished_test(finished: &FinishedTest, gate: &kiss::GateConfig) {
    let status = gated_status(finished, gate);
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        &finished.nodeid,
        finished.duration,
        None,
        true,
    );
    if matches!(status, TestStatus::Failed | TestStatus::TimedOut) && !finished.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&finished.stderr));
    }
}

fn selector_record(
    selector: &str,
    finished: &FinishedTest,
    cache_record: SelectorCacheRecord,
    gate: &kiss::GateConfig,
) -> SelectorExecutionRecord {
    SelectorExecutionRecord {
        selector: selector.to_string(),
        status: gated_status(finished, gate),
        raw_status: Some(finished.status),
        cache_record,
        exit_code: Some(finished.exit_code),
        duration: finished.duration,
    }
}

fn immediate_timeout_record(selector: &str) -> SelectorExecutionRecord {
    let status = TestStatus::TimedOut;
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        selector,
        Duration::ZERO,
        None,
        false,
    );
    SelectorExecutionRecord {
        selector: selector.to_string(),
        status,
        raw_status: None,
        cache_record: SelectorCacheRecord::MissUnstored,
        exit_code: Some(124),
        duration: Duration::ZERO,
    }
}

#[cfg(target_os = "linux")]
fn selected_pytest_runner() -> PytestRunner {
    kiss::rpytest_runner::forkserver_pytest_runner()
}

#[cfg(not(target_os = "linux"))]
fn selected_pytest_runner() -> PytestRunner {
    kiss::rpytest_runner::subprocess_pytest_runner()
}

#[cfg(test)]
#[path = "run_test.rs"]
mod tests;
