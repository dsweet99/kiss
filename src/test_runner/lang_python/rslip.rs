use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::PytestRunner;
use kiss::rslip::{
    CacheStatus as PyCacheStatus, Rslip, RslipBatchProgress, RslipError, RslipOutcome, RslipRequest,
};

use crate::test_runner::last_status::{python_last_status_identity, record_statuses};
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};

#[cfg(test)]
use super::rslip_request::python_version_supports_rslip;
#[cfg(test)]
pub(crate) use super::rslip_request::timeout_for_selector;
use super::rslip_request::timeout_for_selector_with_gate;
pub(crate) use super::rslip_request::{detect_rslip_versions, rslip_request_from_parts};

fn rslip_worker_cap() -> usize {
    kiss::TestSectionConfig::load().num_jobs_pytest.max(1)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
    content_fingerprint: Option<String>,
    gate: &kiss::GateConfig,
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
            gate: gate.clone(),
        },
        selected_rslip_pytest_runner(),
    )
}

struct RslipBatchArgs<'a> {
    repo_root: &'a Path,
    selectors: &'a [String],
    extra: &'a [String],
    force_rerun: bool,
    force_rerun_selectors: &'a [String],
    jobs: usize,
    content_fingerprint: Option<String>,
    gate: kiss::GateConfig,
}

fn run_rslip_selectors_with_runner(
    args: RslipBatchArgs<'_>,
    runner: PytestRunner,
) -> Result<SelectorExecutionSummary, String> {
    assert!(args.jobs > 0, "jobs must be greater than zero");
    let jobs = args.jobs.clamp(1, rslip_worker_cap());
    if jobs < args.jobs {
        crate::test_runner::emit_test_progress(&format!(
            "kiss test: rslip workers={jobs} (capped from {})",
            args.jobs
        ));
    }
    let (python_version, pytest_version) = detect_rslip_versions(args.repo_root)?;
    let identity = python_last_status_identity(&python_version, &pytest_version, args.extra);
    let force_set: BTreeSet<&str> = args
        .force_rerun_selectors
        .iter()
        .map(|selector| selector.as_str())
        .collect();

    let gate = &args.gate;

    let mut template = rslip_request_from_parts(
        args.repo_root,
        "",
        args.extra,
        &python_version,
        &pytest_version,
        false,
        gate,
    )?;
    template.content_fingerprint = args.content_fingerprint;
    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    let (reqs, runnable_selectors) = partition_rslip_requests(
        PartitionInput {
            selectors: args.selectors,
            template: &template,
            force_rerun: args.force_rerun,
            force_set: &force_set,
            gate,
        },
        &mut summary,
        &mut statuses,
    );
    let rslip = Rslip::new(runner);
    let results = rslip.run_or_reuse_many_bounded_with_progress(reqs, jobs, |event| {
        if let RslipBatchProgress::Prepared { elapsed, .. } = &event {
            crate::test_runner::emit_stage_time("rslip_prepare", *elapsed);
        }
        persist_rslip_progress_statuses(args.repo_root, &identity, &runnable_selectors, &event);
        handle_rslip_batch_progress(event, &runnable_selectors, gate);
    });
    for (selector, result) in runnable_selectors.iter().zip(results) {
        record_rslip_selector_result(selector, result, gate, &mut summary, &mut statuses);
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
    statuses: &mut Vec<(String, kiss::rpytest_runner::TestStatus)>,
) -> (Vec<RslipRequest>, Vec<String>) {
    let mut reqs = Vec::new();
    let mut runnable = Vec::new();
    for selector in input.selectors {
        let timeout = timeout_for_selector_with_gate(input.gate, selector);

        if timeout.is_zero() {
            record_immediate_timeout(selector, summary, statuses);
            continue;
        }
        let mut req = input.template.clone();
        req.nodeid = selector.clone();

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
    statuses: &mut Vec<(String, kiss::rpytest_runner::TestStatus)>,
) {
    let status = kiss::rpytest_runner::TestStatus::TimedOut;
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        selector,
        Duration::ZERO,
        None,
        false,
    );
    statuses.push((selector.to_string(), status));
    summary.record(SelectorExecutionRecord {
        selector: selector.to_string(),
        status,
        raw_status: None,
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
    statuses: &mut Vec<(String, kiss::rpytest_runner::TestStatus)>,
) {
    match result {
        Ok(outcome) => {
            let raw = outcome.status;
            let effective = crate::test_runner::status_labels::apply_unit_test_time_limit(
                raw,
                &outcome.nodeid,
                outcome.duration,
                gate,
            );
            statuses.push((outcome.nodeid.clone(), raw));
            summary.record(SelectorExecutionRecord {
                selector: outcome.nodeid.clone(),
                status: effective,
                raw_status: Some(raw),
                cache_record: if outcome.cache_status == PyCacheStatus::Hit {
                    SelectorCacheRecord::Hit
                } else if raw != kiss::rpytest_runner::TestStatus::Passed
                    || outcome.coverage.files.is_empty()
                {
                    SelectorCacheRecord::MissUnstored
                } else {
                    SelectorCacheRecord::MissStored
                },
                exit_code: outcome.exit_code,
                duration: outcome.duration,
            });
        }

        Err(_) => {
            statuses.push((
                selector.to_string(),
                kiss::rpytest_runner::TestStatus::Failed,
            ));
            summary.record(SelectorExecutionRecord {
                selector: selector.to_string(),
                status: kiss::rpytest_runner::TestStatus::Failed,
                raw_status: None,
                cache_record: SelectorCacheRecord::MissUnstored,
                exit_code: Some(1),
                duration: Duration::ZERO,
            });
        }
    }
}

fn persist_rslip_progress_statuses(
    repo_root: &Path,
    identity: &crate::test_runner::last_status::LastStatusIdentity,
    selectors: &[String],
    event: &RslipBatchProgress,
) {
    let RslipBatchProgress::SelectorFinalized { outcomes } = event else {
        return;
    };
    let statuses: Vec<(String, kiss::rpytest_runner::TestStatus)> = outcomes
        .iter()
        .filter_map(|(index, result)| match result {
            Ok(outcome)
                if matches!(
                    outcome.status,
                    kiss::rpytest_runner::TestStatus::Failed
                        | kiss::rpytest_runner::TestStatus::TimedOut
                ) =>
            {
                Some((outcome.nodeid.clone(), outcome.status))
            }
            Err(_) => selectors
                .get(*index)
                .map(|selector| (selector.clone(), kiss::rpytest_runner::TestStatus::Failed)),
            _ => None,
        })
        .collect();
    let _ = record_statuses(repo_root, kiss::Language::Python, identity, &statuses);
}

fn handle_rslip_batch_progress(
    event: RslipBatchProgress,
    selectors: &[String],
    gate: &kiss::GateConfig,
) {
    match event {
        RslipBatchProgress::Prepared {
            cache_hits,
            cache_misses,
            elapsed: _,
        } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: rslip prepared hits={cache_hits} misses={cache_misses}"
            ));
        }
        RslipBatchProgress::SelectorFinalized { outcomes } => {
            emit_finalized_outcomes(outcomes, selectors, gate);
        }
        RslipBatchProgress::CachedStatusDump { body } => {
            emit_progress_lines(&body);
        }
        RslipBatchProgress::TestsRemaining { remaining } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: tests_remaining={remaining}"
            ));
        }
    }
}

fn emit_finalized_outcomes(
    outcomes: Vec<(usize, Result<RslipOutcome, RslipError>)>,
    selectors: &[String],
    gate: &kiss::GateConfig,
) {
    for (index, result) in outcomes {
        match result {
            Ok(outcome) => print_rslip_outcome(&outcome, gate),
            Err(err) => {
                let selector = selectors
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("<unknown>");
                if rslip_protocol_is_quiet_timeout(&err) {
                    let timeout = timeout_for_selector_with_gate(gate, selector);
                    crate::test_runner::emit_test_status(&format!(
                        "TIMEOUT: {selector} ({:.2}s)",
                        timeout.as_secs_f64()
                    ));
                } else {
                    crate::test_runner::emit_test_status(&format!(
                        "FAIL: {selector} (rslip error)"
                    ));
                    eprintln!("{}", format_rslip_error(err));
                }
            }
        }
    }
}

fn emit_progress_lines(body: &str) {
    for line in body.lines() {
        if !line.is_empty() {
            crate::test_runner::emit_test_progress(line);
        }
    }
}

#[cfg(target_os = "linux")]
fn selected_rslip_pytest_runner() -> PytestRunner {
    kiss::rpytest_runner::forkserver_pytest_runner()
}

#[cfg(not(target_os = "linux"))]
fn selected_rslip_pytest_runner() -> PytestRunner {
    kiss::rpytest_runner::subprocess_pytest_runner()
}

fn print_rslip_outcome(outcome: &RslipOutcome, gate: &kiss::GateConfig) {
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        &outcome.nodeid,
        outcome.duration,
        gate,
    );
    let cache_tag = match outcome.cache_status {
        PyCacheStatus::Hit => Some("cached"),
        PyCacheStatus::MissStored => None,
    };
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        &outcome.nodeid,
        outcome.duration,
        cache_tag,
        cache_tag.is_none(),
    );
    if matches!(
        status,
        kiss::rpytest_runner::TestStatus::Failed | kiss::rpytest_runner::TestStatus::TimedOut
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

fn rslip_protocol_is_quiet_timeout(err: &RslipError) -> bool {
    matches!(
        err,
        RslipError::Runner(kiss::rpytest_runner::PytestRunError::Protocol(message))
            if message.contains("module batch result missing")
                || message.contains("module batch timed out")
    )
}

#[cfg(test)]
#[path = "rslip_test.rs"]
mod tests;

#[cfg(test)]
#[path = "rslip_jobs_test.rs"]
mod jobs_tests;

#[cfg(test)]
#[path = "rslip_sla_test.rs"]
mod sla_tests;
