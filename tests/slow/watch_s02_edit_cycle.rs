#![cfg(unix)]

use crate::support::scenario::{
    Reply, Scenario, assert_waits_for_watcher, kiss, skip_under_coverage,
};

fn assert_cached_full_suite(reply: &Reply, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        summary,
        "{phase}: full-suite scope; {reply:?}"
    );
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail"),
        "{phase}: cached FAIL gets a line; {reply:?}"
    );
    assert!(
        !reply.stdout.contains("PASS"),
        "{phase}: the client ran nothing, so no PASS lines; {reply:?}"
    );
}

fn client_during_edit_cycle(s: &Scenario) {
    let starts = s.starts();
    s.edit_until_testing("lib_a.py", "def f():\n    return 0 + 0\n");
    let reply = kiss(s.root(), &["test"]);
    assert_waits_for_watcher(&reply, "client during cycle");
    assert_cached_full_suite(
        &reply,
        "✗ 2 passed · 1 failed · 0 timed out",
        "during cycle",
    );
    assert_eq!(
        s.take_runs(),
        ["test_pass", "test_slow"],
        "the edit cycle runs the needed tests, not the cached FAIL"
    );
    assert_eq!(s.starts(), starts + 1, "the client must not start a cycle");
}

fn client_after_cycle(
    s: &Scenario,
    edit: impl FnOnce(),
    summary: &str,
    expect_runs: &[&str],
    phase: &str,
) {
    let before = s.starts();
    edit();
    s.wait_idle_after(before + 1);
    s.wait_settled();
    assert_eq!(s.take_runs(), expect_runs, "{phase}: watcher cycle runs");
    let starts = s.starts();
    let reply = kiss(s.root(), &["test"]);
    assert_cached_full_suite(&reply, summary, phase);
    assert!(s.take_runs().is_empty(), "{phase}: client runs nothing");
    assert_eq!(s.starts(), starts, "{phase}: client must not start a cycle");
}

#[test]
fn edits_start_watcher_cycles_and_clients_read_the_cache() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow(0.5);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    client_during_edit_cycle(&s);

    let extra = s.py_test("test_new", "assert f() == 0");
    let add = || s.write("test_c.py", &format!("from lib_a import f\n\n\n{extra}"));
    let summary = "✗ 3 passed · 1 failed · 0 timed out";
    client_after_cycle(&s, add, summary, &["test_new"], "add");

    let delete = || std::fs::remove_file(s.root().join("test_c.py")).unwrap();
    let summary = "✗ 2 passed · 1 failed · 0 timed out";
    client_after_cycle(&s, delete, summary, &[], "delete");
    assert!(watch.still_running(), "watcher must keep running");
}
