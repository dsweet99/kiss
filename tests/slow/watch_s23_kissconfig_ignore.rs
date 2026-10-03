#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

fn assert_excluded(reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 1 passed · 1 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(
        !reply.stdout.contains("test_skip") && !reply.stderr.contains("test_skip"),
        "{phase}: the ignored test has no line; {reply:?}"
    );
}

#[test]
fn kissconfig_ignore_prefix_excludes_tests_with_and_without_watcher() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    let config = s.replace_in(".kissconfig", "[test]\n", "[test]\nignore = [\"skipme\"]\n");
    s.write(".kissconfig", &config);
    let skipped = s.py_test("test_skip", "assert f() == 1");
    s.write(
        "skipme/test_skip.py",
        &format!("from lib_a import f\n\n\n{skipped}"),
    );
    s.commit();

    let plain = kiss(s.root(), &["test"]);
    assert_excluded(&plain, "plain run");
    assert_eq!(
        s.take_runs(),
        ["test_fail", "test_pass"],
        "plain run: ignored test never runs"
    );

    let mut watch = s.start_watch();
    s.wait_settled();
    let reply = kiss(s.root(), &["test"]);
    assert_excluded(&reply, "watcher reply");
    assert!(
        !s.take_runs().iter().any(|name| name == "test_skip"),
        "watcher: ignored test never runs"
    );
    assert!(watch.still_running(), "watcher keeps running");
}
