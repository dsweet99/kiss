#![cfg(unix)]

use crate::support::scenario::{Scenario, kiss, skip_under_coverage};
use crate::support::watch_proc::write_kissconfig_with_threshold;

const UNION: [&str; 3] = ["test", "tests/unit", "src/pkg/mod.py"];

fn py_module(s: &Scenario, import: &str, test: &str, body: &str) -> String {
    format!("{import}\n\n\n{}", s.py_test(test, body))
}

fn write_fixture(s: &Scenario) {
    s.write(".gitignore", ".kiss/\n__pycache__/\ntarget/\n");
    s.write("conftest.py", "");
    s.write("lib_a.py", "def f():\n    return 0\n");
    s.write("lib_b.py", "def g():\n    return 1\n");
    s.write("src/__init__.py", "");
    s.write("src/pkg/__init__.py", "");
    s.write("src/pkg/mod.py", "def p():\n    return 1\n");
    let a = "from lib_a import f";
    s.write(
        "tests/unit/test_u.py",
        &py_module(s, a, "test_unit", "assert f() == 0"),
    );
    s.write(
        "tests/other/test_o.py",
        &py_module(s, a, "test_out", "assert f() == 0"),
    );
    let p = "from src.pkg.mod import p";
    s.write(
        "tests/other/test_p.py",
        &py_module(s, p, "test_mod", "assert p() == 1"),
    );
    let b = "from lib_b import g";
    s.write(
        "tests/other/test_f.py",
        &py_module(s, b, "test_fail", "assert g() == 2"),
    );
    write_kissconfig_with_threshold(s.root(), 0.5, 0);
}

fn assert_union_reply(s: &Scenario, phase: &str) {
    let starts = s.starts();
    let reply = kiss(s.root(), &UNION);
    assert_eq!(reply.code, Some(0), "{phase}: {reply:?}");
    assert_eq!(
        reply.summary(),
        "✓ 2 passed · 0 failed · 0 timed out",
        "{phase}: {reply:?}"
    );
    assert!(
        !reply.stdout.contains("tests/other/test_"),
        "{phase}: {reply:?}"
    );
    assert!(s.take_runs().is_empty(), "{phase}: answered from the cache");
    assert_eq!(
        s.starts(),
        starts,
        "{phase}: a client does not start a cycle"
    );
}

fn assert_rejected(s: &Scenario, args: &[&str], needle: &str) {
    let requests = s.requests();
    let reply = kiss(s.root(), args);
    assert_eq!(reply.code, Some(2), "{args:?}: {reply:?}");
    assert!(
        reply.stderr.contains(needle),
        "{args:?}: want {needle:?}; {reply:?}"
    );
    assert_eq!(
        s.requests(),
        requests,
        "{args:?}: rejected before contacting the watcher"
    );
    assert!(s.take_runs().is_empty(), "{args:?}: runs nothing");
}

#[test]
fn several_targets_form_a_union_and_bad_arguments_are_rejected() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    write_fixture(&s);
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    s.take_runs();

    assert_union_reply(&s, "no edits");
    let with_dashdash = [&UNION[..], &["--", "-k", "parse"]].concat();
    assert_rejected(&s, &with_dashdash, "arguments after -- are not accepted");
    assert_rejected(
        &s,
        &["test", "tests/does_not_exist.py"],
        "tests/does_not_exist.py",
    );

    let before = s.starts();
    s.write("lib_a.py", "def f():\n    return 0 + 0\n");
    s.wait_idle_after(before + 1);
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["test_out", "test_unit"],
        "the cycle runs needed tests inside and outside the union"
    );
    assert_union_reply(&s, "after edit");

    let reply = kiss(s.root(), &["test"]);
    assert_eq!(reply.code, Some(1), "{reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 3 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert!(watch.still_running(), "watcher keeps running");
}
