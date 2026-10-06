#![cfg(unix)]

use crate::support::scenario::{
    Scenario, ctrl_c, kiss, skip_under_kiss_test, spawn_kiss_in_own_group,
};

const SUMMARY: &str = "✗ 2 passed · 1 failed · 0 timed out";

#[test]
fn interrupted_plain_run_is_resumed_by_the_next_run() {
    if skip_under_kiss_test() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow();
    s.commit();
    assert_eq!(kiss(s.root(), &["test"]).code, Some(1), "cold run");
    s.take_runs();

    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    let mut plain = spawn_kiss_in_own_group(s.root(), &["test"]);
    s.wait_for_marker("test_slow");
    ctrl_c(&mut plain);
    s.take_runs();

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "{reply:?}");
    assert_eq!(reply.summary(), SUMMARY, "full suite; {reply:?}");
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail"),
        "{reply:?}"
    );
    assert!(
        reply.stdout.contains("PASS: test_a.py::test_slow"),
        "{reply:?}"
    );
    let runs = s.take_runs();
    assert!(
        runs.iter().any(|run| run == "test_slow"),
        "the unrecorded test runs again: {runs:?}"
    );
    assert!(
        !runs.iter().any(|run| run == "test_fail"),
        "a recorded FAIL is not rerun merely because it failed: {runs:?}"
    );

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "cache intact: {reply:?}");
    assert_eq!(reply.summary(), SUMMARY, "cache intact: {reply:?}");
    assert!(s.take_runs().is_empty(), "nothing is still needed");
}
