#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};
use crate::support::watch_proc::write_kissconfig_with_threshold;

fn write_fixture(s: &Scenario) {
    let m = s.marker();
    let m = m.to_str().unwrap();
    s.write(".gitignore", ".kiss/\n__pycache__/\ntarget/\n");
    s.write("pytest.ini", "[pytest]\naddopts = --doctest-modules\n");
    s.write(
        "lib_a.py",
        &format!(
            "def f():\n    \"\"\"\n    >>> open({m:?}, \"a\").write(\"doctest_lib\\\\n\") and f()\n    1\n    \"\"\"\n    return 0\n"
        ),
    );
    s.write(
        "test_a.py",
        &format!(
            "\"\"\"\n>>> open({m:?}, \"a\").write(\"doctest_test\\\\n\") and 2\n3\n\"\"\"\nfrom lib_a import f\n\n\n{}",
            s.py_test("test_pass", "assert f() == 0")
        ),
    );
    s.rust_crate(
        &format!(
            "/// ```\n/// use std::io::Write;\n/// let mut fh = std::fs::OpenOptions::new().create(true).append(true).open({m:?}).unwrap();\n/// writeln!(fh, \"doctest_rs\").unwrap();\n/// assert_eq!(demo::value(), 99);\n/// ```\npub fn value() -> u32 {{\n    1\n}}\n"
        ),
        &[("rs_pass", "assert_eq!(demo::value(), 1);")],
    );
    write_kissconfig_with_threshold(s.root(), 0.5, 0);
}

fn assert_doctests_left_out(s: &Scenario, reply: &Reply, phase: &str) {
    assert_eq!(
        reply.code,
        Some(0),
        "{phase}: doctest failures do not count; {reply:?}"
    );
    assert_eq!(
        reply.summary(),
        "✓ 2 passed · 0 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(
        !reply.stdout.to_lowercase().contains("doctest"),
        "{phase}: {reply:?}"
    );
    let runs = s.take_runs();
    assert!(
        !runs.iter().any(|run| run.starts_with("doctest")),
        "{phase}: doctests must not run: {runs:?}"
    );
}

#[test]
fn doctests_are_neither_run_nor_counted() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    write_fixture(&s);
    s.commit();

    let reply = kiss(s.root(), &["test"]);
    assert!(
        reply.stdout.contains("PASS: test_a.py::test_pass"),
        "{reply:?}"
    );
    assert!(
        reply.stdout.contains("PASS: tests/it.rs::rs_pass"),
        "{reply:?}"
    );
    assert_doctests_left_out(&s, &reply, "plain run");

    let mut watch = s.start_watch();
    s.wait_settled();
    let runs = s.take_runs();
    assert!(
        !runs.iter().any(|run| run.starts_with("doctest")),
        "watcher cycles must not run doctests: {runs:?}"
    );
    let reply = kiss(s.root(), &["test"]);
    assert_doctests_left_out(&s, &reply, "watcher reply");
    assert!(watch.still_running(), "watcher keeps running");
}
