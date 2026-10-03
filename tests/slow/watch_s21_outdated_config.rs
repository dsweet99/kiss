#![cfg(unix)]

use crate::support::scenario::{
    Reply, Scenario, assert_waits_for_watcher, finish, kiss, skip_under_coverage, spawn_kiss,
};

const WARNING: &str = "kiss test-watch: running with outdated configs";
const SUMMARY: &str = "✗ 3 passed · 1 failed · 0 timed out";

fn assert_startup_scope(reply: &Reply, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), SUMMARY, "{phase}: {reply:?}");
    assert!(
        reply.stdout.contains("FAIL test_b.py::test_fail") && !reply.stdout.contains("test_skip"),
        "{phase}: startup ignore list still applies; {reply:?}"
    );
}

fn wait_idle_outdated(s: &Scenario) {
    loop {
        s.wait_log("an idle watcher that has warned", |text| {
            text.trim_end()
                .ends_with(&format!("kiss test: Waiting\n{WARNING}"))
        });
        let before = s.log_text();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if s.log_text() == before {
            return;
        }
    }
}

#[test]
fn config_change_reruns_under_startup_configs_and_warns() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_with_slow(0.5);
    let config = s.replace_in(".kissconfig", "[test]\n", "[test]\nignore = [\"skipme\"]\n");
    s.write(".kissconfig", &config);
    let skipped = s.py_test("test_skip", "assert f() == 1");
    s.write(
        "skipme/test_skip.py",
        &format!("from lib_a import f\n\n\n{skipped}"),
    );
    s.write(
        "pyproject.toml",
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    s.rust_crate(
        "pub fn value() -> u32 {\n    1\n}\n",
        &[("rs_pass", "assert_eq!(demo::value(), 1);")],
    );
    s.commit();

    let mut watch = s.start_watch();
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        ["rs_pass", "test_fail", "test_pass", "test_slow"]
    );
    let before = kiss(s.root(), &["test"]);
    assert_startup_scope(&before, "before any change");
    assert!(
        !before.stderr.contains(WARNING),
        "no warning before notice: {before:?}"
    );

    s.write(
        ".kissconfig",
        &config.replace("ignore = [\"skipme\"]\n", ""),
    );
    let pyproject = std::fs::read_to_string(s.root().join("pyproject.toml")).unwrap();
    s.write("pyproject.toml", &format!("{pyproject}# changed\n"));
    let cargo = std::fs::read_to_string(s.root().join("Cargo.toml")).unwrap();
    s.write("Cargo.toml", &format!("{cargo}# changed\n"));
    s.wait_log("the config rerun running tests", |text| {
        text.rsplit("kiss test: Starting")
            .next()
            .is_some_and(|tail| tail.starts_with(&format!("\n{WARNING}")))
    });
    let slow_started = || {
        std::fs::read_to_string(s.marker())
            .unwrap_or_default()
            .lines()
            .any(|line| line == "test_slow")
    };
    s.wait_log("test_slow running, or the rerun finished", |text| {
        slow_started()
            || text
                .trim_end()
                .ends_with(&format!("kiss test: Waiting\n{WARNING}"))
    });
    assert!(
        slow_started(),
        "the rerun runs the tests again; log={}",
        s.log_text()
    );
    let during = finish(spawn_kiss(s.root(), &["test"]));
    assert_waits_for_watcher(&during, "client during the rerun");
    assert_startup_scope(&during, "client during the rerun");
    assert!(
        during.stderr.contains(WARNING),
        "rerun reply carries the warning: {during:?}"
    );
    assert_eq!(
        s.take_runs(),
        ["rs_pass", "test_fail", "test_pass", "test_slow"],
        "the rerun reuses no cached result and keeps the startup ignore list"
    );

    wait_idle_outdated(&s);
    let starts = s.starts();
    let warnings = s.log_text().matches(WARNING).count();
    let after = kiss(s.root(), &["test"]);
    assert_startup_scope(&after, "client after the rerun");
    assert_eq!(after.stderr.matches(WARNING).count(), 1, "{after:?}");
    assert!(
        s.take_runs().is_empty(),
        "the client is answered from the rerun's cache"
    );
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert_eq!(s.starts(), starts, "a client starts no cycle");
    assert_eq!(
        s.log_text().matches(WARNING).count(),
        warnings,
        "the idle watcher does not repeat the warning"
    );
    assert!(watch.still_running(), "watcher keeps running");
}
