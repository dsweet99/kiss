#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};

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

fn seeded_python_repo() -> tempfile::TempDir {
    crate::common::fresh_seeded_python_repo()
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

#[test]
fn oneshot_reports_missing_rustc_path() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "python_nested_observed.rs:51:python_nested_observed";
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", target]);
    assert_reports_missing_target(ok, &stdout, &stderr, target, "path not found");
}

#[test]
fn oneshot_reports_missing_rs_file() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "bad_path.rs";
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", target]);
    assert_reports_missing_target(ok, &stdout, &stderr, target, "file not found");
}

#[test]
fn oneshot_reports_lang_mismatch() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let target = "test_lib.py";
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", "--lang", "rust", target]);
    assert_reports_missing_target(ok, &stdout, &stderr, target, "--lang selects only rust");
}

#[test]
fn oneshot_rejects_ignore_option() {
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
            "--ignore",
            "test_",
            "test_lib.py",
        ],
    );
    assert!(
        !ok,
        "kiss test --ignore is not an option; stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("unexpected") || stderr.contains("--ignore"),
        "stderr={stderr}"
    );
}

#[test]
fn oneshot_rejects_runner_args_after_double_dash() {
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
        "arguments after -- must fail; stdout={stdout:?} stderr={stderr:?}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("arguments after --") || combined.contains("not accepted"),
        "must reject runner args; stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn oneshot_runs_tests() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let (ok, stdout, stderr) = oneshot_args(tmp.path(), &["test", "--lang", "python", "."]);
    assert!(ok, "stdout={stdout:?} stderr={stderr:?}");
    assert!(stdout.contains("PASS"), "stdout={stdout:?}");
}

#[test]
fn overlapping_oneshots_both_succeed() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = seeded_python_repo();
    let spawn = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
        crate::common::scrub_parent_coverage_env(&mut cmd);
        crate::common::preserve_toolchain_homes(&mut cmd);
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        cmd.args(["test", "--lang", "python", "test_lib.py"])
            .current_dir(tmp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut a = spawn();
    let mut b = spawn();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
}
