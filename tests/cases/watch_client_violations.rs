#![cfg(unix)]

use std::process::Command;
use std::time::Duration;

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{start_watch, write_kissconfig_with_threshold};

#[test]
fn oneshot_surfaces_watcher_coverage_violations() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(
        tmp.path().join("lib.py"),
        "def f():\n    return 0\ndef unused():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
    write_kissconfig_with_threshold(tmp.path(), 1.0, 90);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    std::thread::sleep(Duration::from_secs(8));

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
        stdout.contains("kiss test: Planning"),
        "oneshot must plan locally; stdout={stdout:?}"
    );
    assert!(
        stdout.contains("VIOLATION:test_coverage:"),
        "T finish_with_coverage must show VIOLATION lines; stdout={stdout:?} stderr={stderr:?}"
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
