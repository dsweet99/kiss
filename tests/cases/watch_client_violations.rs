#![cfg(unix)]

use std::process::Command;

use crate::support::watch_proc::{start_watch, wait_watch_idle_cycle, write_kissconfig_with_threshold};

#[test]
fn oneshot_surfaces_watcher_coverage_violations() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    std::fs::write(
        tmp.path().join("lib.py"),
        "def f():\n    return 0\ndef unused():\n    return 1\n",
    )
    .unwrap();
    write_kissconfig_with_threshold(tmp.path(), 0.2, 90);
    // Partial coverage seed: covered f() only — threshold 90 still fails on unused().
    crate::common::seed_python_runtime_coverage(
        tmp.path(),
        &[("test_lib.py::test_f", vec![("lib.py", vec![1, 2])])],
    );

    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    wait_watch_idle_cycle(tmp.path());

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot T with violations");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected non-zero; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !stdout.contains("kiss test: Planning"),
        "oneshot must echo the watcher instead of planning locally; stdout={stdout:?}"
    );
    assert!(
        stdout.contains("VIOLATION:test_coverage:"),
        "oneshot must echo watcher coverage VIOLATION lines; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !stdout.lines().any(|l| l.trim() == "FAIL"),
        "bare FAIL should not replace VIOLATION report; stdout={stdout:?}"
    );
    assert!(
        !stdout.contains("watcher cycle complete"),
        "stdout={stdout:?}"
    );
}
