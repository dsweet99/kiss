#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{
    start_watch, start_watch_logged, wait_watch_idle_cycle, write_kissconfig_with_threshold,
};

fn write_kissconfig(root: &Path, settle: f64) {
    write_kissconfig_with_threshold(root, settle, 0);
}

fn assert_reports_missing_target(ok: bool, stdout: &str, stderr: &str, target: &str, needle: &str) {
    assert!(
        !ok,
        "missing target must fail; stdout={stdout:?} stderr={stderr:?}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains(needle) && combined.contains(target),
        "must report the bad path; stdout={stdout:?} stderr={stderr:?}"
    );
}

fn assert_reports_missing_rustc_path(ok: bool, stdout: &str, stderr: &str, target: &str) {
    assert_reports_missing_target(ok, stdout, stderr, target, "path not found");
}

fn seeded_python_repo() -> tempfile::TempDir {
    crate::common::fresh_seeded_python_watch_repo()
}

fn oneshot_target(dir: &Path, target: &str) -> (bool, String, String) {
    oneshot_args(dir, &["test", target])
}

fn oneshot_args(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.env("PYTHONDONTWRITEBYTECODE", "1");
    let output = cmd.args(args).current_dir(dir).output().expect("oneshot");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn assert_reports_lang_mismatch(ok: bool, stdout: &str, stderr: &str, target: &str) {
    assert_reports_missing_target(ok, stdout, stderr, target, "--lang selects only rust");
}

fn assert_reports_ignore(ok: bool, stdout: &str, stderr: &str, target: &str) {
    assert_reports_missing_target(ok, stdout, stderr, target, "--ignore prefix");
}

#[test]
fn oneshot_reports_missing_rustc_path_without_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "python_nested_observed.rs:51:python_nested_observed";
    let (ok, stdout, stderr) = oneshot_target(tmp.path(), target);
    assert_reports_missing_rustc_path(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_reports_missing_rustc_path_with_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);

    let target = "python_nested_observed.rs:51:python_nested_observed";
    let (ok, stdout, stderr) = oneshot_target(tmp.path(), target);
    assert_reports_missing_rustc_path(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_reports_missing_rs_file_without_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "bad_path.rs";
    let (ok, stdout, stderr) = oneshot_target(tmp.path(), target);
    assert_reports_missing_target(ok, &stdout, &stderr, target, "file not found");
}

#[test]
fn oneshot_reports_missing_rs_file_with_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    let target = "bad_path.rs";
    let (ok, stdout, stderr) = oneshot_target(tmp.path(), target);
    assert_reports_missing_target(ok, &stdout, &stderr, target, "file not found");
}

#[test]
fn oneshot_reports_lang_mismatch_without_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "test_lib.py";
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", "--lang", "rust", target]);
    assert_reports_lang_mismatch(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_reports_lang_mismatch_with_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    let target = "test_lib.py";
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", "--lang", "rust", target]);
    assert_reports_lang_mismatch(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_reports_ignore_without_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "test_lib.py";
    let (ok, stdout, stderr) = oneshot_args(
        tmp.path(),
        &["test", "--lang", "python", "--ignore", "test_", target],
    );
    assert_reports_ignore(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_reports_ignore_with_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    let target = "test_lib.py";
    let (ok, stdout, stderr) = oneshot_args(
        tmp.path(),
        &["test", "--lang", "python", "--ignore", "test_", target],
    );
    assert_reports_ignore(ok, &stdout, &stderr, target);
}

#[test]
fn oneshot_extra_k_filter_without_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let (ok, stdout, stderr) = oneshot_args(
        tmp.path(),
        &[
            "test",
            "--lang",
            "python",
            ".",
            "--",
            "-k",
            "does_not_match",
        ],
    );
    assert!(
        !ok,
        "empty -k selection must fail; stdout={stdout:?} stderr={stderr:?}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("NO COVERING TESTS") || combined.contains("incomplete"),
        "must not recap a passing suite; stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn oneshot_extra_k_filter_with_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();
    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    let (ok, stdout, stderr) = oneshot_args(
        tmp.path(),
        &[
            "test",
            "--lang",
            "python",
            "test_lib.py",
            "--",
            "-k",
            "does_not_match",
        ],
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("NO COVERING TESTS") || combined.contains("incomplete") || !ok,
        "must apply extra -k; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !combined.contains("1 passed") && !combined.contains("✓ 1 passed"),
        "must not recap the last passing suite; stdout={stdout:?} stderr={stderr:?}"
    );
}

fn assert_watcher_oneshot_report(stdout: &str) {
    assert!(
        !stdout.contains("watcher cycle complete"),
        "stdout={stdout:?}"
    );
    assert!(
        !stdout.lines().any(|l| l.trim() == "FAIL"),
        "bare FAIL must not be the report; stdout={stdout:?}"
    );
}

fn official_report_body(stdout: &str) -> String {
    let lines: Vec<_> = stdout
        .lines()
        .filter(|line| !line.starts_with("kiss: "))
        .collect();
    format!("{}\n", lines.join("\n"))
}

fn watch_cycle_count(log: &Path) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .matches("kiss test: Starting")
        .count()
}

#[test]
fn warm_commit_client_does_not_start_another_watcher_cycle() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let root = tmp.path();
    std::fs::write(root.join("lib.py"), "def f():\n    return 0\n\n").unwrap();
    crate::common::seed_python_runtime_coverage(
        root,
        &[("test_lib.py::test_f", vec![("lib.py", vec![1, 2])])],
    );
    commit_all(root, "covered change");
    let log = PathBuf::from(root).join("watch.log");
    let _watch = start_watch_logged(root, &["test", "--watch"], &log);
    wait_watch_idle_cycle(root);
    let before = watch_cycle_count(&log);

    let (workspace_ok, workspace_stdout, workspace_stderr) = oneshot_args(root, &["test"]);
    assert!(
        workspace_ok,
        "workspace client failed: stdout={workspace_stdout:?} stderr={workspace_stderr:?}"
    );
    assert_watcher_oneshot_report(&workspace_stdout);
    let workspace_report = official_report_body(&workspace_stdout);
    let watcher_log = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        workspace_report
            .lines()
            .all(|line| watcher_log.lines().any(|watcher_line| watcher_line == line)),
        "workspace client must return the watcher official report body: watcher={watcher_log:?} client={workspace_report:?}"
    );

    let (commit_ok, commit_stdout, commit_stderr) = oneshot_args(root, &["test", "commit"]);
    assert!(
        commit_ok,
        "commit client failed: stdout={commit_stdout:?} stderr={commit_stderr:?}"
    );
    assert_watcher_oneshot_report(&commit_stdout);
    assert!(
        workspace_stdout.contains("passed") && commit_stdout.contains("passed"),
        "clients must echo cached report summaries: workspace={workspace_stdout:?} commit={commit_stdout:?}"
    );
    assert_eq!(
        watch_cycle_count(&log),
        before,
        "warm workspace and commit clients must not start another watcher cycle; log={}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
}

#[test]
fn oneshot_defers_to_idle_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::locked_seeded_python_watch_repo();

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    // First oneshot primes (and defers to) the watcher; avoid a separate idle nudge cycle.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.env("PYTHONDONTWRITEBYTECODE", "1");
    let output = cmd
        .args(["test", "--lang", "python", "test_lib.py"])
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
    assert_watcher_oneshot_report(&stdout);
    assert!(
        stdout.contains("passed")
            && stdout.contains("failed")
            && stdout.contains("timed out"),
        "oneshot must print a summary from the watcher; stdout={stdout:?}"
    );
}

// Client oneshot-during-settle is covered in-process by
// `test_runner::watch::session_nudge_test::nudge_while_waiting_skips_settle`
// (force nudge skips the quiet period without a multi-second watch e2e).

#[test]
fn oneshot_after_dirty_source_echoes_fail_and_exit() {
    struct Restore<'a>(&'a Path, String);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = std::fs::write(self.0, &self.1);
        }
    }
    let locked = crate::common::locked_seeded_python_watch_repo();
    let root = locked.path();
    let lib = root.join("lib.py");
    let _restore = Restore(&lib, std::fs::read_to_string(&lib).unwrap());
    std::fs::write(&lib, "def f():\n    return 1\n").unwrap();
    let _watch = start_watch(
        root,
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    let mut oneshot = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut oneshot);
    crate::common::preserve_toolchain_homes(&mut oneshot);
    let output = oneshot
        .args(["test", "--lang", "python", "test_lib.py::test_f"])
        .current_dir(root)
        .output()
        .expect("oneshot after dirty source");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "dirty fail must yield non-zero exit; stdout={stdout:?} stderr={stderr:?}"
    );
    assert_watcher_oneshot_report(&stdout);
    assert!(
        stdout.contains("FAIL:") || stdout.contains("failed"),
        "dirty watch path must echo FAIL summary; stdout={stdout:?}"
    );
}

#[test]
fn no_watcher_oneshot_still_runs_tests() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::fresh_seeded_python_watch_repo();

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
    assert!(!stdout.contains("waiting for watcher"), "stdout={stdout:?}");
    assert!(
        stdout.contains("PASS:") || stdout.contains("PASS"),
        "stdout={stdout:?}"
    );
}

#[test]
fn overlapping_oneshots_without_watcher_both_execute() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();

    let mut a_cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut a_cmd);
    crate::common::preserve_toolchain_homes(&mut a_cmd);
    a_cmd.env("PYTHONDONTWRITEBYTECODE", "1");
    let mut a = a_cmd
        .args(["test", "--lang", "python", "test_lib.py"])
        .current_dir(tmp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut b_cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut b_cmd);
    crate::common::preserve_toolchain_homes(&mut b_cmd);
    b_cmd.env("PYTHONDONTWRITEBYTECODE", "1");
    let mut b = b_cmd
        .args(["test", "--lang", "python", "test_lib.py"])
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
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "import time\nfrom lib import f\n\ndef test_f():\n    time.sleep(0.01)\n    assert f() == 0\n",
    )
    .unwrap();

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    std::thread::sleep(Duration::from_millis(15));
    let mut output_cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut output_cmd);
    crate::common::preserve_toolchain_homes(&mut output_cmd);
    let output = output_cmd
        .args(["test", "--lang", "python", "test_lib.py::test_f"])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot during long cycle");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_watcher_oneshot_report(&stdout);
}

#[test]
fn oneshot_with_coverage_gate_defers_to_watcher() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    // Seeded fixture already has coverage; only raise the threshold for this case.
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    write_kissconfig_with_threshold(tmp.path(), 0.02, 1);

    let _watch = start_watch(tmp.path(), &["test", "--watch", "--lang", "python", "."]);
    wait_watch_idle_cycle(tmp.path());
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
    assert_watcher_oneshot_report(&stdout);
    assert!(!stdout.contains("waiting for watcher"), "stdout={stdout:?}");
    assert!(
        !stderr.contains("missing or stale/incompatible population"),
        "stderr={stderr:?}"
    );
    assert!(stdout.contains("PASS"), "stdout={stdout:?}");
}

#[test]
fn stale_generation_repaired_on_watcher_not_client() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    // Persistent seeded fixture (threshold 0) — avoid cold git/seed/python version probes.
    let tmp = crate::common::fresh_seeded_python_watch_repo();

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );

    std::fs::write(
        tmp.path().join("lib.py"),
        "def f():\n    return 0\n# fingerprint-drift\n",
    )
    .unwrap();

    let mut drift_cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::preserve_toolchain_homes(&mut drift_cmd);
    crate::common::scrub_parent_coverage_env(&mut drift_cmd);
    let output = drift_cmd
        .args(["test", "--lang", "python", "test_lib.py::test_f"])
        .current_dir(tmp.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("oneshot T after fingerprint drift");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_watcher_oneshot_report(&stdout);
    assert!(
        output.status.success(),
        "T+W after a source edit must match no-W coverage success; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !stderr.contains("generation identity mismatch"),
        "T must restamp locally instead of fail-closing coverage; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stdout.contains("PASS:") || stdout.contains("✓"),
        "T must still run tests locally after fingerprint drift; stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn watcher_reloads_kissconfig_threshold_change() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    use crate::support::watch_proc::start_watch_logged;
    use std::time::{Duration, Instant};

    let tmp = crate::common::fresh_seeded_python_watch_repo();
    std::fs::write(
        tmp.path().join("lib.py"),
        "def f():\n    return 0\ndef unused():\n    return 1\n",
    )
    .unwrap();
    write_kissconfig_with_threshold(tmp.path(), 0.005, 0);

    let log = tmp.path().join("watch.log");
    let mut watch = start_watch_logged(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "."],
        &log,
    );
    let ready = Instant::now() + Duration::from_secs(5);
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("kiss test: Waiting") {
            break;
        }
        assert!(watch.still_running(), "watcher died early; log={text}");
        assert!(Instant::now() < ready, "initial cycle not idle; log={text}");
        std::thread::sleep(Duration::from_millis(10));
    }
    write_kissconfig_with_threshold(tmp.path(), 0.005, 90);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("VIOLATION:test_coverage:") {
            break;
        }
        assert!(
            watch.still_running(),
            "watcher exited before threshold reload; log={text}"
        );
        assert!(
            Instant::now() < deadline,
            "timed out waiting for threshold reload VIOLATION; log={text}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", "--lang", "python", "."]);
    assert!(
        !ok,
        "expected coverage fail after reload; stdout={stdout:?} stderr={stderr:?}"
    );
    assert_watcher_oneshot_report(&stdout);
    assert!(
        stdout.contains("VIOLATION:test_coverage:") || stderr.contains("VIOLATION:test_coverage:"),
        "reloaded threshold must produce coverage VIOLATION; stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn oneshot_idle_watcher_prints_local_fail_not_bare_fail() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "def test_f():\n    assert False\n",
    )
    .unwrap();
    write_kissconfig(tmp.path(), 0.01);
    commit_all(tmp.path(), "init");

    let _watch = start_watch(
        tmp.path(),
        &["test", "--watch", "--lang", "python", "test_lib.py"],
    );
    wait_watch_idle_cycle(tmp.path());

    let mut fail_cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut fail_cmd);
    crate::common::preserve_toolchain_homes(&mut fail_cmd);
    let output = fail_cmd
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .output()
        .expect("oneshot failing python");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected failure; stdout={stdout:?} stderr={stderr:?}"
    );
    assert_watcher_oneshot_report(&stdout);
    assert!(
        stdout.contains("FAIL:") || stdout.contains("FAIL tests/") || stdout.contains("FAIL test_"),
        "failing python oneshot must print FAIL; stdout={stdout:?}"
    );
}
