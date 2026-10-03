#![cfg(unix)]

use crate::support::scenario::{Scenario, assert_watch_usage_errors, kiss, skip_under_coverage};

fn watch_state(s: &Scenario) -> (Vec<u8>, Vec<u8>) {
    let kiss_dir = s.root().join(".kiss");
    (
        std::fs::read(kiss_dir.join("watch").join("session.json")).unwrap(),
        std::fs::read(kiss_dir.join("test_last_status.json")).unwrap_or_default(),
    )
}

#[test]
fn second_watch_is_refused_and_options_are_usage_errors() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();
    let before = watch_state(&s);
    let starts = s.starts();

    let second = kiss(s.root(), &["test-watch"]);
    assert_eq!(second.code, Some(2), "second bare watch; {second:?}");
    assert!(
        second
            .stderr
            .contains("error: kiss test-watch: watcher already running (pid "),
        "{second:?}"
    );
    assert_watch_usage_errors(s.root());

    assert!(s.take_runs().is_empty(), "no refused command may run tests");
    assert_eq!(s.starts(), starts, "no refused command may start a cycle");
    assert_eq!(s.requests(), 0, "no refused command may reach the watcher");
    assert!(
        watch_state(&s) == before,
        "session and last results stay unchanged"
    );

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "{reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 1 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert_eq!(
        s.requests(),
        1,
        "the first watcher answers the plain client"
    );
    assert!(s.take_runs().is_empty(), "the plain client runs nothing");
    assert!(watch.still_running(), "the first watcher must keep running");
}
