#![cfg(unix)]

use crate::support::scenario::{
    Reply, Scenario, assert_repeats, finish, kiss, skip_under_coverage, spawn_kiss,
};

const EDIT: &str = "def f():\n    return 0 + 0\n";

fn assert_fail_retried(reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 0 passed · 1 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(
        reply.stdout.contains("test_b.py::test_fail"),
        "{phase}: {reply:?}"
    );
}

fn retry_waits_for_plain_run(s: &Scenario) {
    s.write("lib_a.py", EDIT);
    let first = spawn_kiss(s.root(), &["test"]);
    s.wait_for_run();
    let retry = finish(spawn_kiss(s.root(), &["test", "--retry-bad", "test_b.py"]));
    let first = finish(first);
    assert_eq!(first.code, Some(1), "first: {first:?}");
    assert_repeats(
        &retry,
        "kiss test: waiting for kiss test",
        "retry-bad behind plain run",
    );
    assert_fail_retried(&retry, "retry-bad behind plain run");
    let order = std::fs::read_to_string(s.marker()).unwrap();
    assert_eq!(
        order.lines().collect::<Vec<_>>(),
        ["test_pass", "test_slow", "test_fail"],
        "no overlap: the retry runs only after the plain run, and only the FAIL test"
    );
    s.take_runs();
    let after = kiss(s.root(), &["test"]);
    assert_eq!(
        after.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "cache intact: {after:?}"
    );
    assert!(s.take_runs().is_empty(), "cache intact: nothing reruns");
}

#[test]
fn retry_bad_waits_for_plain_run_and_skips_all_pass_target() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow();
    s.commit();
    kiss(s.root(), &["test"]);
    s.take_runs();

    retry_waits_for_plain_run(&s);

    let all_pass = kiss(s.root(), &["test", "--retry-bad", "test_a.py"]);
    assert_eq!(all_pass.code, Some(0), "all-PASS target: {all_pass:?}");
    assert_eq!(
        all_pass.summary(),
        "✓ 2 passed · 0 failed · 0 timed out",
        "{all_pass:?}"
    );
    assert!(
        !all_pass.stdout.contains("PASS"),
        "nothing ran, so no PASS lines: {all_pass:?}"
    );
    assert!(s.take_runs().is_empty(), "all-PASS target runs nothing");
}
