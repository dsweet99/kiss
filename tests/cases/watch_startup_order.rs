#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::watch_proc::{
    spawn_watch, start_watch, wait_watch_idle_cycle, wait_watch_session,
};

fn skip_under_llvm_profile() -> bool {
    std::env::var_os("LLVM_PROFILE_FILE").is_some()
}

fn assert_oneshot_clean(stdout: &str, stderr: &str, status_ok: bool) {
    assert!(
        status_ok,
        "oneshot must succeed; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !stderr.contains("session is not ready"),
        "startup race must not surface a missing session; stderr={stderr:?}"
    );
    assert!(
        !stderr.contains("already running"),
        "oneshot must not take the watch lock; stderr={stderr:?}"
    );
}

fn assert_local_oneshot_ok(stdout: &str, stderr: &str, status_ok: bool) {
    assert_oneshot_clean(stdout, stderr, status_ok);
    assert!(
        stdout.contains("kiss test: Planning"),
        "oneshot without a watcher must plan locally; stdout={stdout:?} stderr={stderr:?}"
    );
}

fn assert_watcher_oneshot_ok(stdout: &str, stderr: &str, status_ok: bool) {
    assert_oneshot_clean(stdout, stderr, status_ok);
    assert!(
        stdout.contains("PASS") || stdout.contains("passed"),
        "oneshot must echo watcher pass/fail/timeout results; stdout={stdout:?}"
    );
}

fn assert_oneshot_local_or_watcher(stdout: &str, stderr: &str, status_ok: bool) {
    assert_oneshot_clean(stdout, stderr, status_ok);
    let planned = stdout.contains("kiss test: Planning");
    let echoed = stdout.contains("PASS") || stdout.contains("passed");
    assert!(
        planned || echoed,
        "oneshot must plan locally or echo the watcher; stdout={stdout:?} stderr={stderr:?}"
    );
}

fn run_planning_oneshot(dir: &Path) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    let output = cmd
        .args(["test", "--dry-run", "--lang", "python", "test_lib.py"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(dir)
        .output()
        .expect("planning oneshot T");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_oneshot(dir: &Path) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    let output = cmd
        .args(["test", "--lang", "python", "test_lib.py"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(dir)
        .output()
        .expect("oneshot T");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn finish_oneshot(child: std::process::Child) -> (bool, String, String) {
    let output = child.wait_with_output().expect("wait oneshot T");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn assert_json_under_kiss_parses(root: &Path) {
    let kiss = root.join(".kiss");
    let mut stack = vec![kiss];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_json = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            if !is_json {
                continue;
            }
            let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|e| {
                panic!("corrupt JSON cache {}: {e}", path.display());
            });
        }
    }
}

fn prepare_repo(sleep_secs: f64) -> tempfile::TempDir {
    // Overlap races need an exclusive tree; sequential cases use locked fixture.
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    if sleep_secs > 0.0 {
        fs::write(
            tmp.path().join("test_lib.py"),
            format!(
                "import time\nfrom lib import f\n\ndef test_f():\n    time.sleep({sleep_secs})\n    assert f() == 0\n"
            ),
        )
        .unwrap();
    }
    tmp
}

#[test]
fn oneshot_then_watch_sequential() {
    if skip_under_llvm_profile() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();
    let (ok, stdout, stderr) = run_planning_oneshot(tmp.path());
    assert_local_oneshot_ok(&stdout, &stderr, ok);

    let mut watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    assert!(watch.still_running(), "watcher must stay up after oneshot");
    assert_json_under_kiss_parses(tmp.path());
}

#[test]
fn watch_then_oneshot_sequential() {
    if skip_under_llvm_profile() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();
    let mut watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    wait_watch_idle_cycle(tmp.path());
    let (ok, stdout, stderr) = run_oneshot(tmp.path());
    assert_watcher_oneshot_ok(&stdout, &stderr, ok);
    assert!(watch.still_running(), "watcher must stay up after oneshot");
    assert_json_under_kiss_parses(tmp.path());
}

fn spawn_planning_oneshot(dir: &Path) -> std::process::Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.args(["test", "--dry-run", "--lang", "python", "test_lib.py"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn planning oneshot T")
}

#[test]
fn oneshot_and_watch_start_together_oneshot_first() {
    if skip_under_llvm_profile() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();
    let oneshot = spawn_planning_oneshot(tmp.path());
    let mut watch = spawn_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    let (ok, stdout, stderr) = finish_oneshot(oneshot);
    wait_watch_session(tmp.path(), &mut watch);
    assert_oneshot_local_or_watcher(&stdout, &stderr, ok);
    assert!(
        watch.still_running(),
        "watcher must start while oneshot is running"
    );
    assert_json_under_kiss_parses(tmp.path());
}

#[test]
fn oneshot_and_watch_start_together_watch_first() {
    if skip_under_llvm_profile() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();
    let mut watch = spawn_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    let oneshot = spawn_planning_oneshot(tmp.path());
    let (ok, stdout, stderr) = finish_oneshot(oneshot);
    wait_watch_session(tmp.path(), &mut watch);
    assert_oneshot_local_or_watcher(&stdout, &stderr, ok);
    assert!(
        watch.still_running(),
        "watcher must stay up when oneshot starts in the same window"
    );
    assert_json_under_kiss_parses(tmp.path());
}

fn spawn_oneshot_args(dir: &Path, args: &[&str]) -> std::process::Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.args(args)
        .current_dir(dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn oneshot")
}

#[test]
fn overlapping_oneshot_and_watch_leave_usable_cache() {
    if skip_under_llvm_profile() {
        return;
    }
    let tmp = prepare_repo(0.01);
    let t0 = Instant::now();
    let oneshot = spawn_oneshot_args(
        tmp.path(),
        &["test", "--lang", "python", "test_lib.py::test_f"],
    );
    std::thread::sleep(Duration::from_millis(15));
    let mut watch = spawn_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    let (ok, stdout, stderr) = finish_oneshot(oneshot);
    wait_watch_session(tmp.path(), &mut watch);
    assert_oneshot_local_or_watcher(&stdout, &stderr, ok);
    assert!(
        t0.elapsed() < Duration::from_secs(60),
        "overlap must finish promptly; elapsed={:?}",
        t0.elapsed()
    );
    assert!(
        tmp.path()
            .join(".kiss")
            .join("watch")
            .join("session.json")
            .is_file(),
        "overlap must leave a usable watch session"
    );
}
