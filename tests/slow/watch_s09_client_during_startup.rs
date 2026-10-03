#![cfg(unix)]

use crate::support::scenario::{
    Scenario, assert_waits_for_watcher, finish, skip_under_coverage, spawn_kiss,
};

#[test]
fn clients_during_startup_wait_then_read_the_cache() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow(0.5);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_log("the startup cycle running tests", |text| {
        text.contains("tests_remaining=")
    });

    let bare = spawn_kiss(s.root(), &["test"]);
    let narrow = spawn_kiss(s.root(), &["test", "test_b.py"]);
    let bare = finish(bare);
    let narrow = finish(narrow);

    assert_waits_for_watcher(&bare, "bare during startup");
    assert_eq!(bare.code, Some(1), "{bare:?}");
    assert_eq!(
        bare.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{bare:?}"
    );
    assert!(
        !bare.stdout.contains("PASS"),
        "the startup cycle's own lines are not the client's reply; {bare:?}"
    );
    assert_waits_for_watcher(&narrow, "PATH during startup");
    assert_eq!(narrow.code, Some(1), "{narrow:?}");
    assert_eq!(
        narrow.summary(),
        "✗ 0 passed · 1 failed · 0 timed out",
        "{narrow:?}"
    );
    assert!(!narrow.stdout.contains("test_a.py"), "{narrow:?}");

    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass", "test_slow"],
        "one unfiltered startup cycle, no overlapping cycle, no client runs"
    );
    assert!(watch.still_running(), "watcher must keep running");
}
