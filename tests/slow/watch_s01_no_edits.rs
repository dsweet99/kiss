#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

fn assert_full_suite_from_cache(s: &Scenario, reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 2 passed · 2 failed · 0 timed out",
        "{phase}: scope is the full suite, Python and Rust; {reply:?}"
    );
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail") && reply.stdout.contains("rs_fail"),
        "{phase}: cached FAIL tests get lines; {reply:?}"
    );
    assert!(
        !reply.stdout.contains("PASS"),
        "{phase}: cached PASS tests that did not run get no line; {reply:?}"
    );
    assert!(s.take_runs().is_empty(), "{phase}: no test may run");
}

#[test]
fn idle_watcher_answers_bare_and_dot_from_cache_without_running_tests() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    s.rust_crate(
        "pub fn value() -> u32 {\n    1\n}\n",
        &[
            ("rs_pass", "assert_eq!(demo::value(), 1);"),
            ("rs_fail", "assert_eq!(demo::value(), 2);"),
        ],
    );
    s.commit();

    let mut watch = s.start_watch();
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["rs_fail", "rs_pass", "test_fail", "test_pass"],
        "startup cycle runs the full suite"
    );
    let starts = s.starts();

    let bare = kiss(s.root(), &["test"]);
    assert_full_suite_from_cache(&s, &bare, "kiss test");
    let dot = kiss(s.root(), &["test", "."]);
    assert_full_suite_from_cache(&s, &dot, "kiss test .");

    assert_eq!(s.starts(), starts, "a plain client must not start a cycle");
    assert_eq!(s.requests(), 2, "both clients must reach the watcher");
    assert!(watch.still_running(), "watcher must keep running");
}
