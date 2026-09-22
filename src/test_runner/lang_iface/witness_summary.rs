use std::time::Duration;

use kiss::rpytest_runner::TestStatus;

use super::witness::{ExecutionWitness, selector_index};
use super::witness_reuse::gate_violation_from_raw_pass;
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};

pub(crate) fn summary_from_accepted_witness(
    planned_selectors: &[String],
    witness: &ExecutionWitness,
    report_id: impl Fn(&str) -> String,
) -> SelectorExecutionSummary {
    summary_from_witness_statuses(planned_selectors, witness, report_id, true)
}

pub(crate) fn summary_from_witness_statuses(
    planned_selectors: &[String],
    witness: &ExecutionWitness,
    report_id: impl Fn(&str) -> String,
    require_all_passed: bool,
) -> SelectorExecutionSummary {
    let index = selector_index(&witness.selectors);
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    let records =
        planned_witness_records(&planned, witness, &index, &report_id, require_all_passed)
            .expect("witness records include stored durations");
    emit_cached_witness_lines(&records);
    let mut summary = SelectorExecutionSummary::default();
    for (report, status, duration) in records {
        summary.record(SelectorExecutionRecord {
            selector: report,
            status,
            raw_status: None,
            cache_record: SelectorCacheRecord::Hit,
            exit_code: Some(exit_code_for_test_status(status)),
            duration,
        });
    }
    summary
}

fn planned_witness_records(
    planned: &[String],
    witness: &ExecutionWitness,
    index: &std::collections::BTreeMap<&str, usize>,
    report_id: &impl Fn(&str) -> String,
    require_all_passed: bool,
) -> Option<Vec<(String, TestStatus, Duration)>> {
    let mut records = Vec::with_capacity(planned.len());
    for selector in planned {
        let i = index[selector.as_str()];
        let status = witness.statuses[i]
            .to_test_status()
            .unwrap_or(TestStatus::Failed);
        if require_all_passed && !gate_violation_from_raw_pass(witness, i) {
            assert_eq!(status, TestStatus::Passed);
        }
        records.push((
            report_id(selector),
            status,
            Duration::from_nanos(witness.durations_ns[i]?),
        ));
    }
    Some(records)
}

fn emit_cached_witness_lines(records: &[(String, TestStatus, Duration)]) {
    if !crate::test_runner::check_runtime_refresh::test_runner_stdout_enabled() {
        return;
    }
    if records.len() <= 64 {
        for (report, status, duration) in records {
            crate::test_runner::status_labels::print_classified_status_line(
                *status,
                report,
                *duration,
                Some("cached"),
                false,
            );
        }
        return;
    }
    let (passed, failed, timed_out) = count_test_statuses(records);
    print_cached_total("PASS", passed);
    print_cached_total("FAIL", failed);
    print_cached_total("TIMEOUT", timed_out);
    for (report, status, duration) in records {
        match status {
            TestStatus::Failed | TestStatus::TimedOut => {
                crate::test_runner::status_labels::print_classified_status_line(
                    *status,
                    report,
                    *duration,
                    Some("cached"),
                    false,
                );
            }
            TestStatus::Passed => {}
        }
    }
}

fn count_test_statuses(records: &[(String, TestStatus, Duration)]) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for (_, status, _) in records {
        match status {
            TestStatus::Passed => passed += 1,
            TestStatus::Failed => failed += 1,
            TestStatus::TimedOut => timed_out += 1,
        }
    }
    (passed, failed, timed_out)
}

fn print_cached_total(label: &str, count: usize) {
    if count > 0 {
        crate::test_runner::emit_test_progress(&format!("{label} (cached): {count} selectors"));
    }
}

fn exit_code_for_test_status(status: TestStatus) -> i32 {
    match status {
        TestStatus::Passed => 0,
        TestStatus::Failed => 1,
        TestStatus::TimedOut => 124,
    }
}
