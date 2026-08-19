use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::PytestRunner;
use rslip::{
    CacheStatus as PyCacheStatus, Rslip, RslipBatchProgress, RslipError, RslipOutcome, RslipRequest,
};

use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};

use super::rslip_request::timeout_for_selector_with_gate;
pub(crate) use super::rslip_request::{detect_rslip_versions, rslip_request_from_parts};
#[cfg(test)]
pub(crate) use super::rslip_request::timeout_for_selector;
#[cfg(test)]
use super::rslip_request::python_version_supports_rslip;

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
    let jobs = rslip_parallel_jobs(args.jobs);
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
    let mut stdout = std::io::BufWriter::new(std::io::stdout());
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
        &mut stdout,
    );
    let results = Rslip::new(runner).run_or_reuse_many_bounded_with_progress(
        reqs,
        jobs,
        |event| {
            if let RslipBatchProgress::Prepared { elapsed, .. } = &event {
                crate::test_runner::emit_stage_time("rslip_prepare", *elapsed);
            }
            handle_rslip_batch_progress(event, &runnable_selectors, gate, &mut stdout);
        },
    );
    let _ = std::io::Write::flush(&mut stdout);
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
    statuses: &mut Vec<(String, rpytest_runner::TestStatus)>,
    stdout: &mut impl Write,
) -> (Vec<RslipRequest>, Vec<String>) {
    let mut reqs = Vec::new();
    let mut runnable = Vec::new();
    for selector in input.selectors {
        let timeout = timeout_for_selector_with_gate(input.gate, selector);

        if timeout.is_zero() {
            record_immediate_timeout(selector, summary, statuses, stdout);
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
    statuses: &mut Vec<(String, rpytest_runner::TestStatus)>,
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
                } else {
                    SelectorCacheRecord::MissStored
                },
                exit_code: outcome.exit_code,
                duration: outcome.duration,
            });
        }


        Err(_) => {
            statuses.push((selector.to_string(), rpytest_runner::TestStatus::Failed));
            summary.record(SelectorExecutionRecord {
                selector: selector.to_string(),
                status: rpytest_runner::TestStatus::Failed,
                raw_status: None,
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
    gate: &kiss::GateConfig,
    stdout: &mut impl Write,
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
            for (index, result) in outcomes {
                match result {
                    Ok(outcome) => {
                        print_rslip_outcome(&outcome, gate, stdout);
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

const MAX_RSLIP_PARALLEL_JOBS: usize = 12;

fn rslip_parallel_jobs(jobs: usize) -> usize {
    jobs.min(MAX_RSLIP_PARALLEL_JOBS)
}

#[cfg(target_os = "linux")]
fn selected_rslip_pytest_runner() -> PytestRunner {
    rpytest_runner::forkserver_pytest_runner()
}

#[cfg(not(target_os = "linux"))]
fn selected_rslip_pytest_runner() -> PytestRunner {
    rpytest_runner::subprocess_pytest_runner()
}

fn print_rslip_outcome(
    outcome: &RslipOutcome,
    gate: &kiss::GateConfig,
    out: &mut impl std::io::Write,
) {
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        &outcome.nodeid,
        outcome.duration,
        gate,
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

#[cfg(test)]
#[path = "rslip_jobs_test.rs"]
mod jobs_tests;

#[cfg(test)]
#[path = "rslip_sla_test.rs"]
mod sla_tests;
