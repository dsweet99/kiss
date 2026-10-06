use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::PytestRunner;
use kiss::rslip::{
    CacheStatus as PyCacheStatus, Rslip, RslipBatchProgress, RslipError, RslipOutcome, RslipRequest,
};

#[cfg(test)]
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::{SelectorCacheRecord, SelectorExecutionRecord};

#[cfg(test)]
pub(super) use super::rslip_emit::{
    emit_finalized_outcomes, emit_progress_lines, format_rslip_error, print_rslip_outcome,
    rslip_protocol_is_quiet_timeout,
};
use super::rslip_emit::{handle_rslip_batch_progress, status_for_rslip_error};
#[cfg(test)]
use super::rslip_request::python_version_supports_rslip;
#[cfg(test)]
pub(crate) use super::rslip_request::timeout_for_selector;
use super::rslip_request::timeout_for_selector_with_gate;
pub(crate) use super::rslip_request::{detect_rslip_versions, rslip_request_from_parts};

fn rslip_worker_cap() -> Option<usize> {
    kiss::TestSectionConfig::load().num_jobs_pytest_explicit
}

fn clamp_rslip_jobs(requested: usize) -> usize {
    match rslip_worker_cap() {
        Some(cap) => requested.clamp(1, cap.max(1)),
        None => requested,
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
    gate: &kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    let mut records = Vec::new();
    run_rslip_selectors_streaming(
        RslipSelectorsArgs {
            repo_root,
            selectors,
            extra,
            force_rerun,
            force_rerun_selectors,
            jobs,
            gate,
        },
        &mut |record| records.push(record),
    )?;
    Ok(SelectorExecutionSummary::default().with_records(records))
}

pub(crate) struct RslipSelectorsArgs<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) selectors: &'a [String],
    pub(crate) extra: &'a [String],
    pub(crate) force_rerun: bool,
    pub(crate) force_rerun_selectors: &'a [String],
    pub(crate) jobs: usize,
    pub(crate) gate: &'a kiss::GateConfig,
}

pub(crate) fn run_rslip_selectors_streaming(
    args: RslipSelectorsArgs<'_>,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<(), String> {
    run_rslip_selectors_with_runner_streaming(
        RslipBatchArgs {
            repo_root: args.repo_root,
            selectors: args.selectors,
            extra: args.extra,
            force_rerun: args.force_rerun,
            force_rerun_selectors: args.force_rerun_selectors,
            jobs: args.jobs,
            gate: args.gate.clone(),
        },
        selected_rslip_pytest_runner(),
        on_result,
    )
}

struct RslipBatchArgs<'a> {
    repo_root: &'a Path,
    selectors: &'a [String],
    extra: &'a [String],
    force_rerun: bool,
    force_rerun_selectors: &'a [String],
    jobs: usize,
    gate: kiss::GateConfig,
}

#[cfg(test)]
fn run_rslip_selectors_with_runner(
    args: RslipBatchArgs<'_>,
    runner: PytestRunner,
) -> Result<SelectorExecutionSummary, String> {
    let mut records = Vec::new();
    run_rslip_selectors_with_runner_streaming(args, runner, &mut |record| records.push(record))?;
    Ok(SelectorExecutionSummary::default().with_records(records))
}

fn run_rslip_selectors_with_runner_streaming(
    args: RslipBatchArgs<'_>,
    runner: PytestRunner,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<(), String> {
    assert!(args.jobs > 0, "jobs must be greater than zero");
    let jobs = clamp_rslip_jobs(args.jobs);
    if jobs < args.jobs {
        crate::test_runner::emit_test_progress(&format!(
            "kiss test: rslip workers={jobs} (capped from {})",
            args.jobs
        ));
    }
    let (python_version, pytest_version) = detect_rslip_versions(args.repo_root)?;
    let force_set: BTreeSet<&str> = args
        .force_rerun_selectors
        .iter()
        .map(|selector| selector.as_str())
        .collect();

    let gate = &args.gate;

    let template = rslip_request_from_parts(
        args.repo_root,
        "",
        args.extra,
        &python_version,
        &pytest_version,
        false,
        gate,
    )?;
    let (reqs, runnable_selectors) = partition_rslip_requests(
        PartitionInput {
            selectors: args.selectors,
            template: &template,
            force_rerun: args.force_rerun,
            force_set: &force_set,
            gate,
        },
        on_result,
    );
    let rslip = Rslip::new(runner);
    let mut streamed = vec![false; runnable_selectors.len()];
    let results = rslip.run_or_reuse_many_bounded_with_progress(reqs, jobs, |event| {
        match &event {
            RslipBatchProgress::Prepared { elapsed, .. } => {
                crate::test_runner::emit_stage_time("rslip_prepare", *elapsed);
            }
            RslipBatchProgress::SelectorFinalized { outcomes } => {
                for (index, result) in outcomes {
                    if let (Some(selector), Some(seen)) =
                        (runnable_selectors.get(*index), streamed.get_mut(*index))
                        && !*seen
                    {
                        *seen = true;
                        on_result(rslip_selector_record(selector, result, gate));
                    }
                }
            }
            _ => {}
        }
        handle_rslip_batch_progress(event, &runnable_selectors, gate);
    });
    for ((selector, result), seen) in runnable_selectors.iter().zip(results).zip(streamed) {
        if !seen {
            on_result(rslip_selector_record(selector, &result, gate));
        }
    }
    Ok(())
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
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> (Vec<RslipRequest>, Vec<String>) {
    let mut reqs = Vec::new();
    let mut runnable = Vec::new();
    for selector in input.selectors {
        let timeout = timeout_for_selector_with_gate(input.gate, selector);

        if timeout.is_zero() {
            on_result(immediate_timeout_record(selector));
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

fn immediate_timeout_record(selector: &str) -> SelectorExecutionRecord {
    let status = kiss::rpytest_runner::TestStatus::TimedOut;
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

#[cfg(test)]
fn record_rslip_selector_result(
    selector: &str,
    result: Result<RslipOutcome, RslipError>,
    gate: &kiss::GateConfig,
    summary: &mut SelectorExecutionSummary,
) {
    summary.record(rslip_selector_record(selector, &result, gate));
}

fn rslip_selector_record(
    selector: &str,
    result: &Result<RslipOutcome, RslipError>,
    gate: &kiss::GateConfig,
) -> SelectorExecutionRecord {
    match result {
        Ok(outcome) => {
            let raw = outcome.status;
            let effective = crate::test_runner::status_labels::apply_unit_test_time_limit(
                raw,
                &outcome.nodeid,
                outcome.duration,
                gate,
            );
            SelectorExecutionRecord {
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
            }
        }
        Err(err) => {
            let (status, exit_code) = status_for_rslip_error(err);
            SelectorExecutionRecord {
                selector: selector.to_string(),
                status,
                raw_status: None,
                cache_record: SelectorCacheRecord::MissUnstored,
                exit_code: Some(exit_code),
                duration: if status == kiss::rpytest_runner::TestStatus::TimedOut {
                    timeout_for_selector_with_gate(gate, selector)
                } else {
                    Duration::ZERO
                },
            }
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

#[cfg(test)]
#[path = "rslip_test.rs"]
mod tests;

#[cfg(test)]
#[path = "rslip_jobs_test.rs"]
mod jobs_tests;

#[cfg(test)]
#[path = "rslip_sla_test.rs"]
mod sla_tests;
