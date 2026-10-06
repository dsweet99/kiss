use std::path::Path;

use kiss::rust_llvm_cov_runner::{RustCovCacheStatus, RustCoverageBatchResult, RustLlvmCovOutcome};

use crate::test_runner::lang_rust::llvm_cov::error::map_rust_llvm_cov_error;
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};
use crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached;

fn print_rust_llvm_cov_outcome(
    outcome: &RustLlvmCovOutcome,
    report_id: &str,
    gate: &kiss::GateConfig,
) -> kiss::rpytest_runner::TestStatus {
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        report_id,
        outcome.duration,
        gate,
    );
    let (cache_tag, show_duration) = match outcome.cache_status {
        RustCovCacheStatus::Hit => (Some("cached"), false),
        RustCovCacheStatus::MissStored => (None, true),
        RustCovCacheStatus::FreshUnstored => (Some("not cached"), true),
    };
    if cache_tag == Some("cached") && status == kiss::rpytest_runner::TestStatus::Passed {
        return status;
    }
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        report_id,
        outcome.duration,
        cache_tag,
        show_duration,
    );
    if matches!(
        status,
        kiss::rpytest_runner::TestStatus::Failed | kiss::rpytest_runner::TestStatus::TimedOut
    ) && outcome.cache_status != RustCovCacheStatus::Hit
        && let Some(stderr) = &outcome.stderr
        && !stderr.is_empty()
    {
        eprint!("{}", String::from_utf8_lossy(stderr));
    }
    status
}

fn emit_bulk_cached_pass_summary(
    outcomes: &[&RustLlvmCovOutcome],
    report_ids: &std::collections::BTreeMap<String, String>,
    gate: &kiss::GateConfig,
) {
    let mut cached_pass = 0usize;
    for outcome in outcomes {
        if !matches!(outcome.cache_status, RustCovCacheStatus::Hit)
            || outcome.status != kiss::rpytest_runner::TestStatus::Passed
        {
            continue;
        }
        let Ok(report_id) =
            crate::test_runner::runners::require_kiss_test_report_id(report_ids, &outcome.selector)
        else {
            continue;
        };
        if kiss::rust_llvm_cov_runner::live_rust_was_printed(&report_id) {
            continue;
        }
        let effective = crate::test_runner::status_labels::apply_unit_test_time_limit(
            outcome.status,
            &report_id,
            outcome.duration,
            gate,
        );
        if effective == kiss::rpytest_runner::TestStatus::Passed {
            cached_pass += 1;
        }
    }
    if cached_pass > 0 {
        crate::test_runner::emit_test_progress(&format!("PASS {cached_pass} selectors"));
    }
}

fn effective_status_for_completed_outcome(
    outcome: &RustLlvmCovOutcome,
    report_id: &str,
    gate: &kiss::GateConfig,
    emit_each: bool,
) -> kiss::rpytest_runner::TestStatus {
    if kiss::rust_llvm_cov_runner::live_rust_was_printed(report_id) {
        return crate::test_runner::status_labels::apply_unit_test_time_limit(
            outcome.status,
            report_id,
            outcome.duration,
            gate,
        );
    }
    if emit_each
        || !matches!(outcome.cache_status, RustCovCacheStatus::Hit)
        || outcome.status != kiss::rpytest_runner::TestStatus::Passed
    {
        return print_rust_llvm_cov_outcome(outcome, report_id, gate);
    }
    crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        report_id,
        outcome.duration,
        gate,
    )
}

fn record_completed_outcome(
    summary: &mut SelectorExecutionSummary,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
    outcome: &RustLlvmCovOutcome,
    report_id: String,
    effective: kiss::rpytest_runner::TestStatus,
) {
    let raw = outcome.status;
    on_result(SelectorExecutionRecord {
        selector: report_id,
        status: effective,
        raw_status: Some(raw),
        cache_record: match outcome.cache_status {
            RustCovCacheStatus::Hit => SelectorCacheRecord::Hit,
            RustCovCacheStatus::MissStored => SelectorCacheRecord::MissStored,
            RustCovCacheStatus::FreshUnstored => SelectorCacheRecord::MissUnstored,
        },
        exit_code: outcome.exit_code,
        duration: outcome.duration,
    });
    summary.raw_statuses.insert(outcome.selector.clone(), raw);
    summary
        .selector_durations_ns
        .insert(outcome.selector.clone(), outcome.duration.as_nanos() as u64);
}

#[cfg(test)]
pub(crate) fn finish_rust_coverage_batch_result(
    repo_root: &Path,
    result: RustCoverageBatchResult,
    gate: &kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    let mut records = Vec::new();
    let summary =
        finish_rust_coverage_batch_result_streaming(repo_root, result, gate, &mut |record| {
            records.push(record)
        })?;
    Ok(summary.with_records(records))
}

pub(super) fn finish_rust_coverage_batch_result_streaming(
    repo_root: &Path,
    result: RustCoverageBatchResult,
    gate: &kiss::GateConfig,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<SelectorExecutionSummary, String> {
    let mut summary = SelectorExecutionSummary::default();
    summary.record_rust_batch_counters(&result.counters);
    let report_ids = rust_logical_to_kiss_test_ids_cached(repo_root, &[])?;
    let current_completed: Vec<_> = result
        .completed
        .iter()
        .filter(|outcome| report_ids.contains_key(&outcome.selector))
        .collect();
    let emit_each = current_completed.len() <= 64;
    if !emit_each {
        emit_bulk_cached_pass_summary(&current_completed, &report_ids, gate);
    }
    for outcome in current_completed {
        let report_id = crate::test_runner::runners::require_kiss_test_report_id(
            &report_ids,
            &outcome.selector,
        )?;
        let effective =
            effective_status_for_completed_outcome(outcome, &report_id, gate, emit_each);
        record_completed_outcome(&mut summary, on_result, outcome, report_id, effective);
    }
    if let Some(err) = result.batch_error {
        return Err(map_rust_llvm_cov_error(err));
    }
    Ok(summary)
}
