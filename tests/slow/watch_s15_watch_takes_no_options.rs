#![cfg(unix)]

use crate::support::scenario::{Scenario, assert_watch_usage_errors, kiss, skip_under_coverage};

const UNKNOWN_TO_TEST: [&[&str]; 5] = [
    &["--metrics"],
    &["--coverage-all"],
    &["--ignore", "tests/unit/slow"],
    &["--dry-run"],
    &["--watch"],
];

fn session_exists(s: &Scenario) -> bool {
    s.root().join(".kiss/watch/session.json").is_file()
}

fn assert_unknown_options_rejected(s: &Scenario, phase: &str) {
    for extra in UNKNOWN_TO_TEST {
        let args: Vec<&str> = std::iter::once("test")
            .chain(extra.iter().copied())
            .collect();
        let reply = kiss(s.root(), &args);
        assert_eq!(reply.code, Some(2), "{phase} {args:?}: {reply:?}");
        let expected = format!("unexpected argument '{}'", extra[0]);
        assert!(
            reply.stderr.contains(&expected),
            "{phase} {args:?}: {reply:?}"
        );
        assert!(s.take_runs().is_empty(), "{phase} {args:?}: runs nothing");
    }
}

fn assert_accepted_without_watcher(s: &Scenario, args: &[&str], runs: &[&str]) {
    let reply = kiss(s.root(), args);
    assert_eq!(reply.code, Some(1), "{args:?}: normal workflow; {reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 1 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert_eq!(s.take_runs(), runs, "{args:?}");
}

fn assert_rejected_by_watcher(s: &Scenario, args: &[&str], option: &str) {
    let reply = kiss(s.root(), args);
    assert_eq!(reply.code, Some(2), "{args:?}: {reply:?}");
    let expected = format!("{option} is not accepted while kiss test-watch is running");
    assert!(reply.stderr.contains(&expected), "{args:?}: {reply:?}");
    assert!(s.take_runs().is_empty(), "{args:?}: runs nothing");
}

#[test]
fn watch_takes_no_options_and_test_rejects_watch_only_flags() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    std::fs::copy(s.root().join(".kissconfig"), s.root().join("ci.kissconfig")).unwrap();
    s.commit();

    let both = ["test_fail", "test_pass"];
    assert_accepted_without_watcher(&s, &["test", "--config", "ci.kissconfig"], &both);
    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    assert_accepted_without_watcher(&s, &["test", "-j", "4"], &["test_pass"]);
    assert_unknown_options_rejected(&s, "no watcher");
    assert_watch_usage_errors(s.root());
    assert!(
        !session_exists(&s),
        "a usage error must not start a watcher"
    );
    assert!(s.take_runs().is_empty(), "usage errors run nothing");

    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();
    let requests = s.requests();
    assert_watch_usage_errors(s.root());
    assert_unknown_options_rejected(&s, "watcher");
    assert_rejected_by_watcher(&s, &["test", "--config", "ci.kissconfig"], "--config");
    assert_rejected_by_watcher(&s, &["test", "-j", "4"], "-j");
    assert_eq!(s.requests(), requests, "no rejected command is served");

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "{reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 1 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert!(s.take_runs().is_empty(), "the client runs nothing");
    assert!(watch.still_running(), "watcher keeps serving");
}
