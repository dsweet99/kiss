#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

fn assert_summary(reply: &Reply, code: i32, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(code), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{phase}: {reply:?}");
}

fn edit_cycle_runs(s: &Scenario, rel: &str, contents: &str) -> Vec<String> {
    let starts = s.starts();
    s.write(rel, contents);
    s.wait_idle_after(starts + 1);
    s.wait_settled();
    s.take_runs()
}

#[test]
fn python_fail_reruns_when_its_code_changes_with_and_without_watcher() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    s.commit();
    let broken_summary = "✗ 1 passed · 1 failed · 0 timed out";

    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 1, broken_summary, "first run");
    assert_eq!(s.take_runs(), ["test_fail", "test_pass"]);
    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    let other = kiss(s.root(), &["test"]);
    assert_summary(&other, 1, broken_summary, "edit to code the FAIL never ran");
    assert!(other.stdout.contains("FAIL test_b.py::test_fail"), "{other:?}");
    assert_eq!(
        s.take_runs(),
        ["test_pass"],
        "a cached FAIL is not rerun merely because it failed"
    );

    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();
    let runs = edit_cycle_runs(&s, "lib_a.py", "def f():\n    return 0 * 1\n");
    assert_eq!(runs, ["test_pass"], "the cycle leaves the cached FAIL alone");
    let runs = edit_cycle_runs(&s, "lib_b.py", "def g():\n    return 2\n");
    assert_eq!(
        runs,
        ["test_fail"],
        "an edit to code the cached FAIL ran makes it needed"
    );
    let fixed = kiss(s.root(), &["test"]);
    assert_summary(&fixed, 0, "✓ 2 passed · 0 failed · 0 timed out", "after the fix");
    assert!(s.take_runs().is_empty(), "the client runs nothing");
    assert!(watch.still_running(), "watcher keeps running");
    drop(watch);
    let plain = kiss(s.root(), &["test"]);
    assert_summary(&plain, 0, "✓ 2 passed · 0 failed · 0 timed out", "plain run after the watcher");
    assert!(s.take_runs().is_empty(), "the plain run after the watcher runs nothing");
}
