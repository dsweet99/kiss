use std::path::Path;

use crate::support::git::{commit_all, git_command, init_git_repo};

fn write_python_fixture(dir: &Path) {
    std::fs::write(dir.join("lib.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(
        dir.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

fn kiss_test_dry_run(dir: &Path, args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_kiss");
    std::process::Command::new(bin)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("kiss test")
}

fn assert_dry_run_prints_selector(mode: &str, out: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{mode}: kiss test --dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("test_lib.py::test_f"),
        "{mode}: expected selector test_lib.py::test_f in stdout, got {stdout}"
    );
}

#[test]
fn kiss_test_commit_dry_run_prints_expected_selector() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 1\n").unwrap();
    let out = kiss_test_dry_run(tmp.path(), &["test", "commit", "--dry-run"]);
    assert_dry_run_prints_selector("commit", &out);
}

#[test]
fn kiss_test_base_and_main_dry_run_print_expected_selector() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    assert!(
        git_command(tmp.path())
            .args(["checkout", "-b", "feature"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 1\n").unwrap();
    let base_out = kiss_test_dry_run(
        tmp.path(),
        &["test", "base", "--base-branch", "main", "--dry-run"],
    );
    assert_dry_run_prints_selector("base", &base_out);
    let main_out = kiss_test_dry_run(
        tmp.path(),
        &["test", "main", "--main-branch", "main", "--dry-run"],
    );
    assert_dry_run_prints_selector("main", &main_out);
}

#[test]
fn kiss_test_base_dry_run_single_branch_exits_nonzero() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    let out = kiss_test_dry_run(tmp.path(), &["test", "base", "--dry-run"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "base: single-branch dry-run must exit non-zero, stderr={stderr}"
    );
    assert!(
        stderr.contains("--base-branch"),
        "base: stderr must guide --base-branch, got {stderr}"
    );
}
