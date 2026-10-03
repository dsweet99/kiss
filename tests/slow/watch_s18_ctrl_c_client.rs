#![cfg(unix)]

use std::time::Duration;

use crate::support::scenario::{
    Scenario, ctrl_c, kiss, skip_under_coverage, spawn_kiss_in_own_group,
};

fn interrupt_waiting_client(s: &Scenario, edit: &str, client_args: &[&str], phase: &str) {
    let starts = s.starts();
    s.edit_until_testing("lib_a.py", edit);
    let mut client = spawn_kiss_in_own_group(s.root(), client_args);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        matches!(client.try_wait(), Ok(None)),
        "{phase}: the client must still be waiting when interrupted"
    );
    ctrl_c(&mut client);

    s.wait_idle_after(starts + 1);
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["test_pass", "test_slow"],
        "{phase}: the cycle finishes as usual and nothing runs for the departed client"
    );
    assert_eq!(
        s.starts(),
        starts + 1,
        "{phase}: no cycle for the departed client"
    );
    assert!(
        !s.log_text().contains("error"),
        "{phase}: log={}",
        s.log_text()
    );

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(s.take_runs().is_empty(), "{phase}: answered from the cache");
}

#[test]
fn interrupted_clients_are_dropped_and_cycles_finish() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow(0.5);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    interrupt_waiting_client(
        &s,
        "def f():\n    return 0 + 0\n",
        &["test"],
        "plain client",
    );
    interrupt_waiting_client(
        &s,
        "def f():\n    return 0 + 0 + 0\n",
        &["test", "--retry-bad"],
        "retry-bad client",
    );
    assert!(watch.still_running(), "watcher keeps running");
}
