#![cfg(unix)]

use crate::support::scenario::{Scenario, kiss, skip_under_coverage};

fn write_fixture(s: &Scenario) {
    s.python_pass_fail(0.5);
    s.write("lib_c.py", "def h():\n    return 3\n");
    let pass = s.py_test("test_pass", "assert f() == 0");
    let both_a = s.py_test("test_both_a", "assert h() == 3");
    s.write(
        "test_a.py",
        &format!("from lib_a import f\nfrom lib_c import h\n\n\n{pass}{both_a}"),
    );
    let fail = s.py_test("test_fail", "assert g() == 2");
    let both_b = s.py_test("test_both_b", "assert h() == 3");
    s.write(
        "test_b.py",
        &format!("from lib_b import g\nfrom lib_c import h\n\n\n{fail}{both_b}"),
    );
}

fn assert_target_reply(s: &Scenario, target: &str, code: i32, summary: &str) {
    let starts = s.starts();
    let reply = kiss(s.root(), &["test", target]);
    assert_eq!(reply.code, Some(code), "{target}: {reply:?}");
    assert_eq!(
        reply.summary(),
        summary,
        "{target}: scope is the TARGET; {reply:?}"
    );
    assert!(
        !reply.stdout.contains("test_b.py") || target.starts_with("test_b.py"),
        "{target}: sibling tests outside TARGET get no line; {reply:?}"
    );
    assert!(
        s.take_runs().is_empty(),
        "{target}: the client runs nothing"
    );
    assert_eq!(s.starts(), starts, "{target}: the client starts no cycle");
}

fn edit_and_settle(s: &Scenario, rel: &str, body: &str) -> (usize, usize) {
    let starts = s.starts();
    s.write(rel, body);
    s.wait_idle_after(starts + 1);
    s.wait_settled();
    let runs = s.take_runs();
    let inside = runs
        .iter()
        .filter(|name| IN_PATH.contains(&name.as_str()))
        .count();
    (inside, runs.len() - inside)
}

const IN_PATH: [&str; 2] = ["test_pass", "test_both_a"];

#[test]
fn target_replies_are_scoped_while_watcher_keeps_full_suite_cache() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    write_fixture(&s);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();
    let pass_only = "✓ 2 passed · 0 failed · 0 timed out";

    assert_target_reply(&s, "test_a.py", 0, pass_only);
    assert_target_reply(
        &s,
        "test_a.py::test_pass",
        0,
        "✓ 1 passed · 0 failed · 0 timed out",
    );
    assert_target_reply(
        &s,
        "test_b.py::test_fail",
        1,
        "✗ 0 passed · 1 failed · 0 timed out",
    );

    let (inside, outside) = edit_and_settle(&s, "lib_b.py", "def g():\n    return 1 + 0\n");
    assert!(
        inside == 0 && outside > 0,
        "lib_b.py needs only tests outside PATH"
    );
    assert_target_reply(&s, "test_a.py", 0, pass_only);

    let (inside, outside) = edit_and_settle(&s, "lib_c.py", "def h():\n    return 3 + 0\n");
    assert!(
        inside > 0 && outside > 0,
        "lib_c.py needs tests on both sides of PATH"
    );
    assert_target_reply(&s, "test_a.py", 0, pass_only);

    let bare = kiss(s.root(), &["test"]);
    assert_eq!(
        bare.summary(),
        "✗ 3 passed · 1 failed · 0 timed out",
        "full-suite results stay cached; {bare:?}"
    );
    assert!(watch.still_running(), "watcher must keep running");
}
