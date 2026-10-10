#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_kiss_test};

fn assert_summary(reply: &Reply, code: i32, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(code), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{phase}: {reply:?}");
}

#[test]
fn python_source_edit_reruns_population_and_cached_fail_waits_for_it() {
    if skip_under_kiss_test() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail();
    s.commit();
    let broken_summary = "✗ 1 passed · 1 failed · 0 timed out";

    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 1, broken_summary, "first run");
    assert_eq!(s.take_runs(), ["test_fail", "test_pass"]);

    let unchanged = kiss(s.root(), &["test"]);
    assert_summary(&unchanged, 1, broken_summary, "unchanged tree");
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass"],
        "an unchanged tree still runs every selected test"
    );

    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    let other = kiss(s.root(), &["test"]);
    assert_summary(&other, 1, broken_summary, "edit to code the FAIL never ran");
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass"],
        "any Python source edit reruns every Python test"
    );

    s.write("lib_b.py", "def g():\n    return 2\n");
    let fixed = kiss(s.root(), &["test"]);
    assert_summary(
        &fixed,
        0,
        "✓ 2 passed · 0 failed · 0 timed out",
        "after the fix",
    );
    assert_eq!(s.take_runs(), ["test_fail", "test_pass"]);
    let again = kiss(s.root(), &["test"]);
    assert_summary(
        &again,
        0,
        "✓ 2 passed · 0 failed · 0 timed out",
        "run after the fix",
    );
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass"],
        "a later run still executes every selected test"
    );
}
