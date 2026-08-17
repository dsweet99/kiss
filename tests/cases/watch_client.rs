//! Acceptance: one-shot `kiss test` defers to a live `kiss test --watch`.

#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{
    start_watch, wait_watch_idle_cycle, write_kissconfig_with_threshold,
};

fn write_python_fixture(root: &Path) {
    std::fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(
        root.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

fn write_kissconfig(root: &Path, settle: f64) {
    write_kissconfig_with_threshold(root, settle, 0);
}

#[test]
fn oneshot_defers_to_idle_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig(tmp.path(), 1.0);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot T");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "status={:?} stdout={stdout:?} stderr={stderr:?}",
        output.status
    );
    assert!(
        stdout.contains("waiting for watcher"),
        "stdout={stdout:?}"
    );
    assert!(
        stdout.contains("watcher cycle complete"),
        "stdout={stdout:?}"
    );
    let planning_count = stdout.matches("kiss test: Planning").count();
    assert!(
        planning_count == 0,
        "oneshot client must not plan locally; stdout={stdout:?}"
    );
}

#[test]
fn oneshot_during_settle_skips_quiet_period() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig(tmp.path(), 20.0);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());

    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n# touch\n",
    )
    .unwrap();
    let t0 = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot during settle");
    let elapsed = t0.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("waiting for watcher"), "stdout={stdout:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "nudge should skip 20s settle; elapsed={elapsed:?}"
    );
}

#[test]
fn no_watcher_oneshot_still_runs_tests() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig(tmp.path(), 1.0);
    commit_all(tmp.path(), "init");

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot without W");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !stdout.contains("waiting for watcher"),
        "stdout={stdout:?}"
    );
    assert!(stdout.contains("PASS:") || stdout.contains("PASS"), "stdout={stdout:?}");
}

#[test]
fn overlapping_oneshots_without_watcher_both_execute() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig(tmp.path(), 1.0);
    commit_all(tmp.path(), "init");

    let mut a = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut b = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status_a = a.wait().unwrap();
    let status_b = b.wait().unwrap();
    assert!(status_a.success());
    assert!(status_b.success());
}

#[test]
fn oneshot_waits_out_long_inflight_cycle() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "import time\nfrom lib import f\n\ndef test_f():\n    time.sleep(1.5)\n    assert f() == 0\n",
    )
    .unwrap();
    write_kissconfig(tmp.path(), 1.0);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    std::thread::sleep(Duration::from_millis(200));
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot during long cycle");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("waiting for watcher"), "stdout={stdout:?}");
    assert!(
        !stdout.contains("kiss test: Planning"),
        "client must not plan; stdout={stdout:?}"
    );
}

#[test]
fn oneshot_with_coverage_gate_defers_to_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig_with_threshold(tmp.path(), 1.0, 1);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());
    std::thread::sleep(Duration::from_secs(5));

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot T with coverage");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "status={:?} stdout={stdout:?} stderr={stderr:?}",
        output.status
    );
    assert!(
        stdout.contains("waiting for watcher"),
        "stdout={stdout:?}"
    );
    assert!(
        !stderr.contains("missing or stale/incompatible population"),
        "stderr={stderr:?}"
    );
    assert!(
        !stdout.contains("kiss test: Planning"),
        "client must not plan; stdout={stdout:?}"
    );
    assert!(stdout.contains("PASS"), "stdout={stdout:?}");
}

#[test]
fn stale_generation_repaired_on_watcher_not_client() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    write_kissconfig_with_threshold(tmp.path(), 1.0, 1);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());
    std::thread::sleep(Duration::from_secs(5));

    std::fs::write(
        tmp.path().join("lib.py"),
        "def f():\n    return 0\n# fingerprint-drift\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot T after fingerprint drift");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("waiting for watcher"),
        "stdout={stdout:?}"
    );
    assert!(
        !stderr.contains("missing or stale/incompatible population"),
        "T must not own cov/load; stderr={stderr:?}"
    );
    assert!(
        !stdout.contains("kiss test: Planning"),
        "client must not plan; stdout={stdout:?}"
    );
    assert!(
        output.status.success(),
        "status={:?} stdout={stdout:?} stderr={stderr:?}",
        output.status
    );
}

#[test]
fn watcher_reloads_kissconfig_threshold_change() {
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
    // Start with threshold 0 so the first cycle passes despite unused().
    write_kissconfig_with_threshold(tmp.path(), 1.0, 0);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());

    let ok = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot before reload");
    assert!(
        ok.status.success(),
        "pre-reload must pass; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );

    // Raise threshold; watcher should reload and fail coverage on the next cycle.
    write_kissconfig_with_threshold(tmp.path(), 1.0, 90);
    std::thread::sleep(Duration::from_secs(4));

    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot after kissconfig reload");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected coverage fail after reload; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stdout.contains("waiting for watcher"),
        "stdout={stdout:?}"
    );
    assert!(
        stdout.contains("VIOLATION:test_coverage:") || stderr.contains("VIOLATION:test_coverage:"),
        "reloaded threshold must produce coverage VIOLATION; stdout={stdout:?} stderr={stderr:?}"
    );
}
