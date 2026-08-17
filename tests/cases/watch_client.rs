//! Acceptance: one-shot `kiss test` defers to a live `kiss test --watch`.

#![cfg(unix)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

struct WatchProc {
    child: Child,
}

impl Drop for WatchProc {
    fn drop(&mut self) {
        let pid = self.child.id() as i32;
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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

fn write_kissconfig_with_threshold(root: &Path, settle: f64, threshold: u8) {
    std::fs::write(
        root.join(".kissconfig"),
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             orphan_module_enabled = false\n\
             \n\
[test]\n\
             test_coverage_threshold = {threshold}\n\
             watch_settle_seconds = {settle}\n\
             \n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n"
        ),
    )
    .unwrap();
}

#[allow(clippy::zombie_processes)] // WatchProc::Drop always reaps the child.
fn start_watch(dir: &Path, args: &[&str]) -> WatchProc {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watch");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if dir.join(".kiss").join("watch").join("session.json").is_file() {
            return WatchProc { child };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch session not ready");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Wait until the watcher's first cycle has likely finished (warm caches).
fn wait_watch_idle_cycle(dir: &Path) {
    let _ = dir;
    std::thread::sleep(Duration::from_secs(3));
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
    // Client must not re-plan/execute locally (no Planning from T).
    // Watcher may have printed PASS; T prints PASS after cycle complete.
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

    // Touch a test file to enter settle, then immediately oneshot.
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
    // Start T while W may still be in its first long cycle, or nudge during next.
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot during long cycle");
    let elapsed = t0.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("waiting for watcher"), "stdout={stdout:?}");
    // T itself must not run the 3s sleep as a second executor; wall time is from W's cycle.
    assert!(
        !stdout.contains("kiss test: Planning"),
        "client must not plan; stdout={stdout:?}"
    );
    let _ = elapsed;
}

#[test]
fn oneshot_with_coverage_gate_defers_to_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    // Low threshold so the tiny fully-covered fixture can pass.
    write_kissconfig_with_threshold(tmp.path(), 1.0, 1);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
    );
    wait_watch_idle_cycle(tmp.path());
    // Give the watcher time to finish coverage after the first test cycle.
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
    // Allow first cycle's coverage refresh to finish.
    std::thread::sleep(Duration::from_secs(5));

    // Drift the Python source fingerprint while keeping tests green.
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
    // W refreshes in-cycle; combined exit should be success for this tiny fixture.
    assert!(
        output.status.success(),
        "status={:?} stdout={stdout:?} stderr={stderr:?}",
        output.status
    );
}

