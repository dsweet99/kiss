#![cfg(unix)]

use crate::support::scenario::{Scenario, kiss, skip_under_coverage};
use crate::support::watch_proc::write_kissconfig_with_threshold;

fn py_module(s: &Scenario, module: &str, func: &str, test: &str, body: &str) -> String {
    format!("from {module} import {func}\n\n\n{}", s.py_test(test, body))
}

fn write_fixture(s: &Scenario) {
    s.write(".gitignore", ".kiss/\n__pycache__/\ntarget/\ntest_ign.py\n");
    s.write("lib_a.py", "def f():\n    return 0\n");
    s.write("lib_b.py", "def g():\n    return 1\n");
    s.write("lib_e.py", "def e():\n    return 1\n");
    s.write(
        "test_a.py",
        &py_module(s, "lib_a", "f", "test_a", "assert f() == 0"),
    );
    s.write(
        "test_b.py",
        &py_module(s, "lib_b", "g", "test_b", "assert g() == 1"),
    );
    s.write(
        "test_e.py",
        &py_module(s, "lib_e", "e", "test_fail", "assert e() == 2"),
    );
    s.write("test_d.py", &s.py_test("test_del", "assert False"));
    write_kissconfig_with_threshold(s.root(), 0.5, 0);
}

fn make_feature_changes(s: &Scenario) {
    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    s.git(&["commit", "-qam", "change a"]);
    s.write("lib_b.py", "def g():\n    return 1 + 0\n");
    s.write("test_new.py", &s.py_test("test_new", "assert True"));
    s.write("test_ign.py", &s.py_test("test_ign", "assert True"));
    s.git(&["rm", "-q", "test_d.py"]);
}

fn assert_git_reply(s: &Scenario, args: &[&str], code: i32, summary: &str) {
    let starts = s.starts();
    let reply = kiss(s.root(), args);
    assert_eq!(reply.code, Some(code), "{args:?}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{args:?}: scope; {reply:?}");
    assert!(
        !reply.stdout.contains("test_d.py"),
        "{args:?}: deleted test file results are omitted; {reply:?}"
    );
    assert!(
        s.take_runs().is_empty(),
        "{args:?}: answered from the cache"
    );
    assert_eq!(
        s.starts(),
        starts,
        "{args:?}: a client does not start a cycle"
    );
}

#[test]
fn git_targets_are_answered_from_the_cache_for_their_scope() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    write_fixture(&s);
    s.commit();
    s.git(&["branch", "-m", "trunk"]);
    s.git(&["checkout", "-qb", "dev"]);
    s.write("README", "dev\n");
    s.git(&["add", "README"]);
    s.git(&["commit", "-qm", "dev"]);
    s.git(&["checkout", "-qb", "feature"]);
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    make_feature_changes(&s);
    s.wait_running_after(s.starts() + 1);
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["test_a", "test_b", "test_new"],
        "the watcher's own cycle runs what the edits require"
    );

    let pass2 = "✓ 2 passed · 0 failed · 0 timed out";
    let pass3 = "✓ 3 passed · 0 failed · 0 timed out";
    assert_git_reply(&s, &["test", "commit"], 0, pass2);
    assert_git_reply(&s, &["test", "base", "--base-branch", "dev"], 0, pass3);
    assert_git_reply(&s, &["test", "main", "--main-branch", "trunk"], 0, pass3);

    let reply = kiss(s.root(), &["test", "commit", "--main-branch", "trunk"]);
    assert_eq!(reply.code, Some(2), "{reply:?}");
    assert!(
        reply
            .stderr
            .contains("error: kiss test: --main-branch is only valid with kiss test main"),
        "{reply:?}"
    );

    assert_git_reply(&s, &["test"], 1, "✗ 3 passed · 1 failed · 0 timed out");
    assert!(watch.still_running(), "watcher keeps running");
}
