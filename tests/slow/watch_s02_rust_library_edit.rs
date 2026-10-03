#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

const LIB: &str = "pub mod third;\n\npub fn value() -> u32 {\n    1\n}\n\npub fn other() -> u32 {\n    5\n}\n";

fn assert_summary(reply: &Reply, code: i32, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(code), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{phase}: {reply:?}");
}

fn edit_cycle_runs(s: &Scenario, contents: &str) -> Vec<String> {
    let starts = s.starts();
    s.write("src/lib.rs", contents);
    s.wait_idle_after(starts + 1);
    s.wait_settled();
    s.take_runs()
}

#[test]
fn rust_library_edit_reruns_needed_tests_with_and_without_watcher() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.write(".gitignore", ".kiss/\ntarget/\n");
    crate::support::watch_proc::write_kissconfig_with_threshold(s.root(), 0.5, 0);
    s.rust_crate(
        LIB,
        &[
            ("rs_value", "assert_eq!(demo::value(), 1);"),
            ("rs_other", "assert_eq!(demo::other(), 5);"),
            ("rs_third", "assert_eq!(demo::third::third(), 9);"),
        ],
    );
    s.write("src/third.rs", "pub fn third() -> u32 {\n    9\n}\n");
    s.commit();
    let broken_summary = "✗ 2 passed · 1 failed · 0 timed out";

    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 0, "✓ 3 passed · 0 failed · 0 timed out", "first run");
    assert_eq!(s.take_runs(), ["rs_other", "rs_third", "rs_value"]);

    let broken_lib = LIB.replacen("    1\n", "    7\n", 1);
    s.write("src/lib.rs", &broken_lib);
    let broken = kiss(s.root(), &["test"]);
    assert_summary(&broken, 1, broken_summary, "after breaking value()");
    assert!(broken.stdout.contains("rs_value"), "the FAIL has a line: {broken:?}");
    assert!(
        s.take_runs().iter().any(|name| name == "rs_value"),
        "a library edit reruns the test it breaks"
    );
    let cached = kiss(s.root(), &["test"]);
    assert_summary(&cached, 1, broken_summary, "no edit");
    assert!(s.take_runs().is_empty(), "no edit, nothing reruns");

    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();
    let runs = edit_cycle_runs(&s, &broken_lib.replacen("    5\n", "    2 + 3\n", 1));
    assert_eq!(
        runs,
        ["rs_other"],
        "the cycle reruns only the edited function's test; the cached FAIL is not rerun merely because it failed; log={}",
        s.log_text()
    );
    let partial = kiss(s.root(), &["test"]);
    assert_summary(&partial, 1, broken_summary, "after a partial cycle every test stays in the cache");
    assert!(s.take_runs().is_empty(), "the client runs nothing");

    let runs = edit_cycle_runs(&s, &LIB.replacen("    5\n", "    2 + 3\n", 1));
    assert_eq!(
        runs,
        ["rs_value"],
        "fixing value() reruns the cached FAIL that ran it, and nothing else"
    );
    let fixed = kiss(s.root(), &["test"]);
    assert_summary(&fixed, 0, "✓ 3 passed · 0 failed · 0 timed out", "after the fix");
    assert!(s.take_runs().is_empty(), "the client runs nothing");
    assert!(watch.still_running(), "watcher keeps running");
    drop(watch);
    let plain = kiss(s.root(), &["test"]);
    assert_summary(&plain, 0, "✓ 3 passed · 0 failed · 0 timed out", "plain run after the watcher");
    assert!(s.take_runs().is_empty(), "the plain run after the watcher runs nothing");
}

#[test]
fn rust_fail_from_the_first_run_is_left_alone_by_unrelated_edits() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.write(".gitignore", ".kiss/\ntarget/\n");
    crate::support::watch_proc::write_kissconfig_with_threshold(s.root(), 0.5, 0);
    let broken_lib = LIB.replacen("    1\n", "    7\n", 1);
    s.rust_crate(
        &broken_lib,
        &[
            ("rs_value", "assert_eq!(demo::value(), 1);"),
            ("rs_other", "assert_eq!(demo::other(), 5);"),
            ("rs_third", "assert_eq!(demo::third::third(), 9);"),
        ],
    );
    s.write("src/third.rs", "pub fn third() -> u32 {\n    9\n}\n");
    s.commit();
    let broken_summary = "✗ 2 passed · 1 failed · 0 timed out";
    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 1, broken_summary, "first run");
    assert_eq!(s.take_runs(), ["rs_other", "rs_third", "rs_value"]);
    let edited = broken_lib.replacen("    5\n", "    2 + 3\n", 1);
    s.write("src/lib.rs", &edited);
    let unknown = kiss(s.root(), &["test"]);
    assert_summary(&unknown, 1, broken_summary, "first edit");
    s.take_runs();
    s.write("src/lib.rs", &edited.replacen("2 + 3", "3 + 2", 1));
    let other = kiss(s.root(), &["test"]);
    assert_summary(&other, 1, broken_summary, "edit to code the FAIL never ran");
    assert_eq!(
        s.take_runs(),
        ["rs_other"],
        "a cached FAIL is not rerun merely because it failed"
    );
    s.write("src/lib.rs", &LIB.replacen("    5\n", "    3 + 2\n", 1));
    let fixed = kiss(s.root(), &["test"]);
    assert_summary(&fixed, 0, "✓ 3 passed · 0 failed · 0 timed out", "after the fix");
    assert_eq!(s.take_runs(), ["rs_value"], "the fix reruns the FAIL that ran it");
}
