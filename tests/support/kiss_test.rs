use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::support::git::{git_command, init_git_repo};

pub fn kiss_bin() -> PathBuf {
    let compile_time = PathBuf::from(env!("CARGO_BIN_EXE_kiss"));
    if compile_time.is_file() {
        return compile_time;
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in ["target/debug/kiss", "target/llvm-cov-target/debug/kiss"] {
        let candidate = manifest_dir.join(rel);
        if candidate.is_file() {
            return candidate;
        }
    }
    let exe = std::env::current_exe().expect("current test executable path");
    for dir in exe.ancestors() {
        let candidate = dir.join("kiss");
        if candidate.is_file() {
            return candidate;
        }
    }
    compile_time
}

pub fn kiss_command() -> Command {
    Command::new(kiss_bin())
}

pub fn warm_rslip(dir: &Path) {
    let status = Command::new(kiss_bin())
        .current_dir(dir)
        .args(["--defaults", "check", "--lang", "python", "--all", "."])
        .status()
        .expect("kiss check");
    assert!(status.success(), "kiss check should warm rslip cache");
}

pub fn git_commit_all(dir: &Path, message: &str) {
    git_command(dir).args(["add", "."]).status().unwrap();
    git_command(dir)
        .args(["commit", "-m", message])
        .status()
        .unwrap();
}

pub fn init_python_lib_repo(dir: &Path) {
    init_git_repo(dir);
    std::fs::write(dir.join("lib.py"), "def f():\n    return 1\n").unwrap();
    std::fs::write(
        dir.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 1\n",
    )
    .unwrap();
}

pub fn setup_warm_python_lib_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_python_lib_repo(tmp.path());
    git_commit_all(tmp.path(), "init");
    warm_rslip(tmp.path());
    tmp
}

pub fn setup_warm_python_rust_mixed_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_python_lib_repo(tmp.path());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn bump(n: i32) -> i32 { n + 1 }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    git_commit_all(tmp.path(), "init");
    warm_rslip(tmp.path());
    tmp
}

pub fn kiss_test_dry_run(dir: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(kiss_bin());
    cmd.current_dir(dir).args(["test", "commit", "--dry-run"]);
    cmd.args(extra_args);
    cmd.output().expect("kiss test --dry-run")
}

pub fn pytest_lines(stdout: &str) -> Vec<&str> {
    stdout.lines().filter(|l| l.contains("pytest")).collect()
}
