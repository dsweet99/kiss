#![cfg(unix)]

use crate::support::scenario::{Scenario, finish, kiss, skip_under_coverage, spawn_kiss};

const HOLD_SECONDS: usize = 30;
const WAIT_LINE: &str = "kiss test-watch: waiting for kiss test";

fn write_fixture(s: &Scenario) {
    s.python_pass_fail(0.5);
    s.write("lib_c.py", "def h():\n    return 0\n");
    let fast = s.py_test("test_pass", "assert f() == 0");
    let slow = s.py_test("test_slow", "time.sleep(h())");
    s.write(
        "test_a.py",
        &format!("import time\n\nfrom lib_a import f\nfrom lib_c import h\n\n\n{fast}{slow}"),
    );
}

#[test]
fn watcher_started_during_a_plain_run_waits_then_reuses_its_results() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    write_fixture(&s);
    s.commit();
    assert_eq!(kiss(s.root(), &["test"]).code, Some(1), "cold run");
    s.take_runs();

    s.write(
        "lib_c.py",
        &format!("def h():\n    return {HOLD_SECONDS}\n"),
    );
    let plain = spawn_kiss(s.root(), &["test"]);
    s.wait_for_marker("test_slow");
    let mut watch = s.spawn_watch();
    let reply = finish(plain);

    let log = s.log_text();
    let before_start = log.split("kiss test: Starting").next().unwrap_or("");
    let waits = before_start.matches(WAIT_LINE).count();
    assert!(
        waits >= HOLD_SECONDS / 3 - 2,
        "watcher must repeat {WAIT_LINE:?} every 3 s while locked out; got {waits}; log={log}"
    );
    assert!(!log.contains("already running"), "log={log}");
    assert!(
        watch.still_running(),
        "watcher must not give up while waiting"
    );

    assert_eq!(reply.code, Some(1), "plain run: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail"),
        "{reply:?}"
    );
    assert!(
        reply.stdout.contains("PASS: test_a.py::test_slow"),
        "{reply:?}"
    );
    let runs = s.take_runs();
    assert!(runs.iter().any(|run| run == "test_slow"), "{runs:?}");
    assert!(!runs.iter().any(|run| run == "test_fail"), "{runs:?}");

    s.wait_settled();
    assert!(
        s.take_runs().is_empty(),
        "first cycle reuses what the plain run recorded"
    );
    let starts = s.starts();
    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "client: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert!(s.take_runs().is_empty(), "the client runs nothing");
    assert_eq!(s.starts(), starts, "the client does not start a cycle");
    assert!(watch.still_running(), "watcher keeps running");
}
