use std::path::Path;

use kiss::rust_llvm_cov_runner::{RustCovCacheStatus, RustCoverageBatchResult, RustLlvmCovOutcome};

use crate::test_runner::lang_rust::llvm_cov::error::map_rust_llvm_cov_error;
use crate::test_runner::last_status::{LastStatusIdentity, record_statuses};
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
    rust_logical_to_kiss_test_ids,
};

#[allow(dead_code)]
pub(crate) fn cached_summary_from_check_aggregate_population(
    repo_root: &Path,
    selectors: &[String],
    population: &kiss::rust_llvm_cov_runner::RustPopulationState,
) -> Option<SelectorExecutionSummary> {
    if !population
        .entries_fingerprint
        .starts_with("check-aggregate:")
    {
        return None;
    }
    let cache_root = repo_root.join(".kiss").join("rust_llvm_cov_cache");
    let pairs = kiss::rust_llvm_cov_runner::try_load_population_durations(&cache_root, population)?;
    let duration_by_selector: std::collections::BTreeMap<_, _> = pairs.into_iter().collect();
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).ok()?;
    let mut summary = SelectorExecutionSummary::default();
    for selector in selectors {
        let report =
            crate::test_runner::runners::require_kiss_test_report_id(&report_ids, selector).ok()?;
        let duration = duration_by_selector.get(selector).copied()?;
        println!("PASS (cached): {report}");
        summary.record(SelectorExecutionRecord {
            selector: report,
            status: kiss::rpytest_runner::TestStatus::Passed,
            raw_status: None,
            cache_record: SelectorCacheRecord::Hit,
            exit_code: Some(0),
            duration,
        });
    }
    Some(summary)
}

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

pub(crate) fn finish_rust_coverage_batch_result(
    repo_root: &Path,
    identity: &LastStatusIdentity,
    result: RustCoverageBatchResult,
    gate: &kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    let mut summary = SelectorExecutionSummary::default();
    summary.record_rust_batch_counters(&result.counters);
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[])?;
    let mut statuses = Vec::new();
    let emit_each = result.completed.len() <= 64;
    let mut cached_pass = 0usize;
    if !emit_each {
        for outcome in &result.completed {
            if matches!(outcome.cache_status, RustCovCacheStatus::Hit)
                && outcome.status == kiss::rpytest_runner::TestStatus::Passed
            {
                cached_pass += 1;
            }
        }
        if cached_pass > 0 {
            println!("PASS (cached): {cached_pass} selectors");
        }
    }
    for outcome in &result.completed {
        let report_id = crate::test_runner::runners::require_kiss_test_report_id(
            &report_ids,
            &outcome.selector,
        )?;
        let raw = outcome.status;
        let effective = if emit_each
            || !matches!(outcome.cache_status, RustCovCacheStatus::Hit)
            || outcome.status != kiss::rpytest_runner::TestStatus::Passed
        {
            print_rust_llvm_cov_outcome(outcome, &report_id, gate)
        } else {
            crate::test_runner::status_labels::apply_unit_test_time_limit(
                outcome.status,
                &report_id,
                outcome.duration,
                gate,
            )
        };

        statuses.push((outcome.selector.clone(), raw));
        summary.record(SelectorExecutionRecord {
            selector: report_id.clone(),
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
    record_statuses(repo_root, kiss::Language::Rust, identity, &statuses)?;
    if let Some(err) = result.batch_error {
        return Err(map_rust_llvm_cov_error(err));
    }
    Ok(summary)
}
