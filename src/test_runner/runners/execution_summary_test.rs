use super::*;
use rpytest_runner::TestStatus;
use std::time::Duration;

fn passed_miss(selector: &str, duration: Duration) -> SelectorExecutionRecord {
    SelectorExecutionRecord {
        selector: selector.to_string(),
        status: TestStatus::Passed,
        cache_record: SelectorCacheRecord::MissStored,
        exit_code: Some(0),
        duration,
    }
}

#[test]
fn record_updates_max_pass_only_for_fresh_passes() {
    let mut summary = SelectorExecutionSummary::default();
    summary.record(passed_miss("a", Duration::from_millis(100)));
    summary.record(passed_miss("b", Duration::from_millis(250)));
    summary.record(passed_miss("c", Duration::from_millis(50)));
    summary.record(SelectorExecutionRecord {
        selector: "cached".to_string(),
        status: TestStatus::Passed,
        cache_record: SelectorCacheRecord::Hit,
        exit_code: Some(0),
        duration: Duration::from_secs(9),
    });
    summary.record(SelectorExecutionRecord {
        selector: "fail".to_string(),
        status: TestStatus::Failed,
        cache_record: SelectorCacheRecord::MissStored,
        exit_code: Some(1),
        duration: Duration::from_secs(4),
    });
    summary.record(SelectorExecutionRecord {
        selector: "fresh_unstored".to_string(),
        status: TestStatus::Passed,
        cache_record: SelectorCacheRecord::MissUnstored,
        exit_code: Some(0),
        duration: Duration::from_millis(200),
    });

    assert_eq!(summary.total, 6);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.cache_hits, 1);
    assert_eq!(summary.cache_misses, 5);
    assert_eq!(summary.cache_unstored, 1);
    assert_eq!(summary.exit_code, 1);
    assert_eq!(summary.failed_selectors, vec!["fail".to_string()]);
    assert_eq!(
        summary.max_passing_run_duration,
        Duration::from_millis(250)
    );
}

#[test]
fn record_timed_out_counts_as_failed_with_timeout_selector() {
    let mut summary = SelectorExecutionSummary::default();
    summary.record(SelectorExecutionRecord {
        selector: "slow".to_string(),
        status: TestStatus::TimedOut,
        cache_record: SelectorCacheRecord::MissStored,
        exit_code: Some(124),
        duration: Duration::from_secs(2),
    });
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.timed_out_selectors, vec!["slow".to_string()]);
    assert!(summary.failed_selectors.is_empty());
    assert_eq!(summary.exit_code, 124);
}
