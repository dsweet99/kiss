#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{start_watch_logged, write_kissconfig_with_threshold};

const BOTH: [&str; 2] = ["test_a.py", "test_b.py"];
const PASS_ONLY: [&str; 1] = ["test_a.py"];

fn retry_bad(root: &Path, targets: &[&str]) -> (Option<i32>, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    let out = cmd
        .args(["test", "--retry-bad"])
        .args(targets)
        .current_dir(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("run kiss test --retry-bad");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn take_runs(marker: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(marker).unwrap_or_default();
    std::fs::write(marker, "").unwrap();
    text.lines().map(str::to_owned).collect()
}

fn read_log(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn wait_idle_with(log: &Path, needle: &str, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let text = read_log(log);
        if text.matches(needle).count() >= count && text.trim_end().ends_with("kiss test: Waiting")
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "watcher never went idle after {count}x {needle:?}; log={text:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn write_fixture(root: &Path, marker: &Path) {
    let mark = format!(
        "    with open({:?}, \"a\") as fh:\n        fh.write(__name__ + \"\\n\")\n",
        marker.to_str().unwrap()
    );
    std::fs::write(root.join(".gitignore"), ".kiss/\n__pycache__/\n").unwrap();
    std::fs::write(root.join("lib_a.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(root.join("lib_b.py"), "def g():\n    return 1\n").unwrap();
    std::fs::write(
        root.join("test_a.py"),
        format!("from lib_a import f\n\n\ndef test_pass():\n{mark}    assert f() == 0\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("test_b.py"),
        format!("from lib_b import g\n\n\ndef test_fail():\n{mark}    assert g() == 2\n"),
    )
    .unwrap();
    write_kissconfig_with_threshold(root, 5.0, 0);
}

fn assert_fail_retried(reply: &(Option<i32>, String), runs: &[String], phase: &str) {
    let (code, stdout) = reply;
    assert_eq!(runs, ["test_b"], "{phase}: only the FAIL test may run");
    assert_eq!(*code, Some(1), "{phase}: stdout={stdout:?}");
    assert!(
        stdout.contains("FAIL test_b.py::test_fail") && stdout.contains("1 passed · 1 failed"),
        "{phase}: stdout={stdout:?}"
    );
    assert!(
        !stdout.contains("test_a.py::test_pass"),
        "{phase}: PASS test did not run, so it gets no line; stdout={stdout:?}"
    );
}

#[test]
fn retry_bad_runs_only_fail_tests_in_target() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let side = tempfile::TempDir::new().unwrap();
    let marker = side.path().join("runs.txt");
    let log = side.path().join("watch.log");
    init_git_repo(root);
    write_fixture(root, &marker);
    commit_all(root, "init");

    let mut watch = start_watch_logged(root, &["test-watch"], &log);
    wait_idle_with(&log, "1 passed · 1 failed", 1);
    let mut startup = take_runs(&marker);
    startup.sort();
    assert_eq!(startup, ["test_a", "test_b"], "startup cycle runs both");

    let reply = retry_bad(root, &BOTH);
    assert_fail_retried(&reply, &take_runs(&marker), "mixed target");

    let starts = read_log(&log).matches("kiss test: Starting").count();
    let (code, stdout) = retry_bad(root, &PASS_ONLY);
    assert!(take_runs(&marker).is_empty(), "all-PASS target runs nothing");
    assert_eq!(code, Some(0), "all-PASS target; stdout={stdout:?}");
    assert!(stdout.contains("1 passed · 0 failed"), "stdout={stdout:?}");
    assert_eq!(
        read_log(&log).matches("kiss test: Starting").count(),
        starts,
        "all-PASS --retry-bad must not start a cycle"
    );

    std::fs::write(root.join("lib_a.py"), "def f():\n    return 0 + 0\n").unwrap();
    let reply = retry_bad(root, &BOTH);
    assert_fail_retried(&reply, &take_runs(&marker), "before edit cycle");
    wait_idle_with(&log, "PASS: test_a.py::test_pass", 2);
    assert_eq!(take_runs(&marker), ["test_a"], "edit cycle runs only PASS test");
    let reply = retry_bad(root, &BOTH);
    assert_fail_retried(&reply, &take_runs(&marker), "after edit cycle");
    assert!(watch.still_running(), "watcher must keep running");
}
