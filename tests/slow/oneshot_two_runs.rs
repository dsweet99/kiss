#![cfg(unix)]

use crate::support::scenario::{
    Reply, Scenario, assert_repeats, finish, kiss, skip_under_kiss_test, spawn_kiss,
};

const SUMMARY: &str = "✗ 2 passed · 1 failed · 0 timed out";

fn assert_cached_only(reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), SUMMARY, "{phase}: {reply:?}");
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail"),
        "{phase}: {reply:?}"
    );
    assert!(
        !reply.stdout.contains("PASS"),
        "{phase}: nothing ran; {reply:?}"
    );
}

#[test]
fn second_oneshot_waits_for_lock_then_reruns_nothing() {
    if skip_under_kiss_test() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow();
    s.commit();
    let cold = kiss(s.root(), &["test"]);
    assert_eq!(cold.summary(), SUMMARY, "{cold:?}");
    s.take_runs();

    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    let first = spawn_kiss(s.root(), &["test"]);
    s.wait_for_run();
    let second = finish(spawn_kiss(s.root(), &["test"]));
    let first = finish(first);

    assert_eq!(first.code, Some(1), "first: {first:?}");
    assert_eq!(first.summary(), SUMMARY, "first: {first:?}");
    assert!(
        first.stdout.contains("PASS: test_a.py::test_slow")
            && first.stdout.contains("FAIL: test_b.py::test_fail"),
        "first: a Python edit reruns every Python test; {first:?}"
    );
    assert_repeats(&second, "kiss test: waiting for kiss test", "second");
    assert_cached_only(&second, "second");
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass", "test_slow"],
        "each needed test runs once, never against the same cache twice"
    );

    let third = kiss(s.root(), &["test"]);
    assert_cached_only(&third, "third");
    assert!(s.take_runs().is_empty(), "third runs nothing");
}
