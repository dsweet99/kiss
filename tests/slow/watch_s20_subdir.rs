#![cfg(unix)]

use crate::support::scenario::{Scenario, kiss, skip_under_coverage};

#[test]
fn subdirectory_clients_reach_the_root_watcher_with_cwd_scopes() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    let sub = s.py_test("test_s", "assert f() == 0");
    s.write("sub/test_s.py", &format!("from lib_a import f\n\n\n{sub}"));
    let deep = s.py_test("test_d", "assert f() == 0");
    s.write(
        "sub/deep/test_d.py",
        &format!("from lib_a import f\n\n\n{deep}"),
    );
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    let cwd = s.root().join("sub");
    let starts = s.starts();
    let cases: [(&[&str], i32, &str, &str); 3] = [
        (
            &["test"],
            1,
            "✗ 3 passed · 1 failed · 0 timed out",
            "bare: whole repository",
        ),
        (
            &["test", "."],
            0,
            "✓ 2 passed · 0 failed · 0 timed out",
            ". : the subdirectory",
        ),
        (
            &["test", "deep/test_d.py"],
            0,
            "✓ 1 passed · 0 failed · 0 timed out",
            "PATH from cwd",
        ),
    ];
    for (args, code, summary, phase) in cases {
        let requests = s.requests();
        let reply = kiss(&cwd, args);
        assert_eq!(reply.code, Some(code), "{phase}: {reply:?}");
        assert_eq!(reply.summary(), summary, "{phase}: {reply:?}");
        assert_eq!(
            s.requests(),
            requests + 1,
            "{phase}: the root watcher answers"
        );
        assert!(s.take_runs().is_empty(), "{phase}: answered from the cache");
        assert_eq!(s.starts(), starts, "{phase}: no cycle");
    }
    assert!(watch.still_running(), "watcher keeps running");
}
