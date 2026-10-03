#![cfg(unix)]

use std::time::Duration;

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

fn assert_cached_report(reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail"),
        "{phase}: cached FAIL gets a line; {reply:?}"
    );
}

fn interrupted_watcher_recovers(signal: i32) {
    let s = Scenario::new();
    s.python_with_slow(0.5);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    s.wait_for_marker("test_slow");
    assert!(
        watch.signal_and_wait(signal, Duration::from_secs(2)),
        "watcher must exit quickly on signal {signal}"
    );
    s.take_runs();

    let reply = kiss(s.root(), &["test"]);
    assert_cached_report(&reply, "stale session");
    assert!(
        !reply.stderr.contains("waiting for watcher"),
        "a dead watcher must not be waited for; {reply:?}"
    );
    let runs = s.take_runs();
    assert!(
        runs.iter().any(|run| run == "test_slow"),
        "the interrupted, unrecorded test must run again: {runs:?}"
    );
    assert!(
        !runs.iter().any(|run| run == "test_fail"),
        "a recorded FAIL is not rerun merely because it failed: {runs:?}"
    );
    assert!(
        reply.stdout.contains("PASS: test_a.py::test_slow"),
        "a test that ran gets a line; {reply:?}"
    );

    let mut watch = s.start_watch();
    s.wait_settled();
    assert!(
        !s.log_text().contains("already running"),
        "restart must replace the stale session; log={}",
        s.log_text()
    );
    assert!(s.take_runs().is_empty(), "restart cycle reuses the cache");
    let reply = kiss(s.root(), &["test"]);
    assert_cached_report(&reply, "new watcher");
    assert!(s.take_runs().is_empty(), "the client runs nothing");
    assert!(watch.still_running(), "new watcher keeps running");
}

#[test]
fn ctrl_c_watcher_leaves_a_recoverable_session() {
    if skip_under_coverage() {
        return;
    }
    interrupted_watcher_recovers(libc::SIGINT);
}

#[test]
fn killed_watcher_leaves_a_recoverable_session() {
    if skip_under_coverage() {
        return;
    }
    interrupted_watcher_recovers(libc::SIGKILL);
}
