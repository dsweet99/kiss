#![cfg(unix)]

use std::time::Duration;

use crate::support::scenario::{
    Scenario, assert_waits_for_watcher, finish, kiss, skip_under_coverage, spawn_kiss,
};

#[test]
fn an_edit_during_a_cycle_waits_for_its_own_cycle() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow(0.5);
    std::fs::remove_file(s.root().join("test_b.py")).unwrap();
    s.write("lib_c.py", "def h():\n    return 3\n");
    let third = s.py_test("test_c", "assert h() == 3");
    s.write("test_c.py", &format!("from lib_c import h\n\n\n{third}"));
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    let starts = s.starts();
    s.edit_until_testing("lib_a.py", "def f():\n    return 0 + 0\n");
    let client = spawn_kiss(s.root(), &["test"]);
    std::thread::sleep(Duration::from_millis(1000));
    s.write("lib_c.py", "def h():\n    return 4\n");
    let first = finish(client);
    assert_waits_for_watcher(&first, "first client");
    assert_eq!(first.code, Some(0), "first client: {first:?}");
    assert_eq!(
        first.summary(),
        "✓ 3 passed · 0 failed · 0 timed out",
        "first client: answered from the cycle that was running; {first:?}"
    );
    assert!(
        !first.stdout.contains("test_c") && !first.stdout.contains("PASS"),
        "first client: no lines for the second edit or for tests it did not run; {first:?}"
    );

    s.wait_idle_after(starts + 2);
    s.wait_settled();
    assert_eq!(
        s.starts(),
        starts + 2,
        "one cycle per edit, none for the client"
    );
    assert_eq!(
        s.take_runs(),
        ["test_c", "test_pass", "test_slow"],
        "each edit's tests run once, in the watcher's own cycles"
    );

    let later = kiss(s.root(), &["test"]);
    assert_eq!(later.code, Some(1), "later client: {later:?}");
    assert_eq!(
        later.summary(),
        "✗ 2 passed · 1 failed · 0 timed out",
        "{later:?}"
    );
    assert!(later.stdout.contains("FAIL test_c.py::test_c"), "{later:?}");
    assert!(
        s.take_runs().is_empty(),
        "later client: answered from the cache"
    );
    assert_eq!(s.starts(), starts + 2, "later client: no cycle");
    assert!(watch.still_running(), "watcher keeps running");
}
