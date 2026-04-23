mod sync_stats_check_impl;
use sync_stats_check_impl::{
    find_sync_failures, parse_stat_lines, parse_violation_lines, sync_stats_test_body,
};
use std::collections::BTreeMap;
use tempfile::TempDir;

#[test]
fn stats_and_check_agree_on_shared_metrics() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let (stats_stdout, check_stdout, failure_msg) = sync_stats_test_body(tmp.path(), home.path());
    assert!(!parse_stat_lines(&stats_stdout).is_empty(), "no STAT lines:\n{stats_stdout}");
    assert!(!parse_violation_lines(&check_stdout).is_empty(), "no VIOLATION lines:\n{check_stdout}");
    assert!(
        failure_msg.is_empty(),
        "stats/check out of sync:\n\n{failure_msg}\n\n--- stats ---\n{stats_stdout}\n--- check ---\n{check_stdout}"
    );
}

#[test]
fn find_sync_failures_mismatch_when_check_differs() {
    let mut a = BTreeMap::new();
    let mut b = BTreeMap::new();
    a.insert(("m", "f", "n"), 1);
    b.insert(("m", "f", "n"), 2);
    let f = find_sync_failures(&a, &b);
    assert!(f.join("\n").contains("VALUE MISMATCHES"));
}
