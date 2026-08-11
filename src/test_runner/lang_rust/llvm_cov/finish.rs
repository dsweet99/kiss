use std::path::Path;

use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchResult, RustLlvmCovOutcome,
};

use crate::test_runner::last_status::{LastStatusIdentity, record_statuses};
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary, kiss_test_report_id,
    rust_logical_to_kiss_test_ids,
};
use crate::test_runner::lang_rust::llvm_cov::error::map_rust_llvm_cov_error;

#[allow(dead_code)] // check-aggregate warm path retired from production; kept for unit tests
pub(crate) fn cached_summary_from_check_aggregate_population(
    repo_root: &Path,
    selectors: &[String],
    population: &rust_llvm_cov_runner::RustPopulationState,
) -> Option<SelectorExecutionSummary> {
    if !population
        .entries_fingerprint
        .starts_with("check-aggregate:")
    {
        return None;
    }
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    let mut summary = SelectorExecutionSummary::default();
    for selector in selectors {
        let report = kiss_test_report_id(&report_ids, selector);
        println!("PASS (cached): {report}");
        summary.record(SelectorExecutionRecord {
            selector: report,
            status: rpytest_runner::TestStatus::Passed,
            cache_record: SelectorCacheRecord::Hit,
            exit_code: Some(0),
            duration: std::time::Duration::ZERO,
        });
    }
    Some(summary)
}

fn print_rust_llvm_cov_outcome(
    outcome: &RustLlvmCovOutcome,
    report_id: &str,
    gate: &kiss::GateConfig,
) -> rpytest_runner::TestStatus {
    // Limits must use PATH::symbol report ids so patterns like ["rust", 10] match.
    // Logical nextest ids (bare fn / tests::fn) fall through to ["*", 0] otherwise.
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
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        report_id,
        outcome.duration,
        cache_tag,
        show_duration,
    );
    if matches!(
        status,
        rpytest_runner::TestStatus::Failed | rpytest_runner::TestStatus::TimedOut
    ) && outcome.cache_status != RustCovCacheStatus::Hit
        && let Some(stderr) = &outcome.stderr
        && !stderr.is_empty()
    {
        eprint!("{}", String::from_utf8_lossy(stderr));
    }
    status
}

pub(crate) fn finish_rust_coverage_batch_result(
    repo_root: &Path,
    identity: &LastStatusIdentity,
    result: RustCoverageBatchResult,
) -> Result<SelectorExecutionSummary, String> {
    let mut summary = SelectorExecutionSummary::default();
    summary.record_rust_batch_counters(&result.counters);
    let gate = kiss::GateConfig::load();
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    let mut statuses = Vec::new();
    let emit_each = result.completed.len() <= 64;
    let mut cached_pass = 0usize;
    if !emit_each {
        for outcome in &result.completed {
            if matches!(outcome.cache_status, RustCovCacheStatus::Hit)
                && outcome.status == rpytest_runner::TestStatus::Passed
            {
                cached_pass += 1;
            }
        }
        if cached_pass > 0 {
            println!("PASS (cached): {cached_pass} selectors");
        }
    }
    for outcome in &result.completed {
        let report_id = kiss_test_report_id(&report_ids, &outcome.selector);
        let status = if emit_each
            || !matches!(outcome.cache_status, RustCovCacheStatus::Hit)
            || outcome.status != rpytest_runner::TestStatus::Passed
        {
            print_rust_llvm_cov_outcome(outcome, &report_id, &gate)
        } else {
            // Already counted in the collapsed PASS (cached) line above.
            crate::test_runner::status_labels::apply_unit_test_time_limit(
                outcome.status,
                &report_id,
                outcome.duration,
                &gate,
            )
        };
        // last_status keeps nextest logical ids for prior-failure replay.
        statuses.push((outcome.selector.clone(), status));
        summary.record(SelectorExecutionRecord {
            selector: report_id,
            status,
            cache_record: match outcome.cache_status {
                RustCovCacheStatus::Hit => SelectorCacheRecord::Hit,
                RustCovCacheStatus::MissStored => SelectorCacheRecord::MissStored,
                RustCovCacheStatus::FreshUnstored => SelectorCacheRecord::MissUnstored,
            },
            exit_code: outcome.exit_code,
            duration: outcome.duration,
        });
    }
    record_statuses(repo_root, kiss::Language::Rust, identity, &statuses)?;
    if let Some(err) = result.batch_error {
        return Err(map_rust_llvm_cov_error(err));
    }
    Ok(summary)
}
