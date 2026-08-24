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

#[test]
fn kiss_test_non_watch_omits_excess_progress_logging() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    let out = kiss_test_dry_run(tmp.path(), &["test", ".", "--dry-run"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "kiss test . --dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("kiss test: Planning ..."),
        "kiss test must emit an early planning heartbeat, got {stdout}"
    );
    for excess in [
        "kiss test: using cached selectors",
        "kiss test: checking python coverage population",
        "kiss test: checking rust coverage population",
        "kiss test: selected ",
        "kiss test: deciding execution phases",
        "kiss test: deciding python phase",
        "kiss test: discovering python universe",
        "kiss test: deciding rust phase",
        "kiss test: running python population",
    ] {
        assert!(
            !stdout.contains(excess),
            "non-watch kiss test must not emit excess progress {excess:?}, got {stdout}"
        );
    }
}

#[test]
fn kiss_test_dot_prints_final_pass_recap() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    let bin = env!("CARGO_BIN_EXE_kiss");
    let cold = std::process::Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", ".", "--force"])
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test cold");
    let cold_out = String::from_utf8_lossy(&cold.stdout);
    let cold_err = String::from_utf8_lossy(&cold.stderr);
    assert!(
        cold.status.success(),
        "cold kiss test . should pass, stderr={cold_err}, stdout={cold_out}"
    );
    let cold_recap = cold_out
        .lines()
        .rev()
        .find(|line| line.starts_with("✓ ") || line.starts_with("✗ "))
        .unwrap_or("");
    assert!(
        cold_recap.starts_with("✓ ")
            && cold_recap.contains(" passed · ")
            && cold_recap.contains(" total · "),
        "cold run must include pass recap, recap={cold_recap}, stdout={cold_out}"
    );
    assert!(
        cold_out.contains("PASS:") || cold_out.contains("PASS (cached):"),
        "streaming PASS lines must remain: {cold_out}"
    );

    let warm = std::process::Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", "."])
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test warm");
    let warm_out = String::from_utf8_lossy(&warm.stdout);
    let warm_err = String::from_utf8_lossy(&warm.stderr);
    assert!(
        warm.status.success(),
        "warm kiss test . should pass, stderr={warm_err}, stdout={warm_out}"
    );
    let warm_recap = warm_out
        .lines()
        .rev()
        .find(|line| line.starts_with("✓ ") || line.starts_with("✗ "))
        .unwrap_or("");
    assert!(
        warm_recap.starts_with("✓ ")
            && warm_recap.contains(" passed · ")
            && warm_recap.ends_with("0s max pass"),
        "warm cache hits must keep pass count and 0s max pass, recap={warm_recap}, stdout={warm_out}"
    );
}

#[test]
fn kiss_test_python_failure_prints_failed_recap_line() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 1\n",
    )
    .unwrap();
    commit_all(tmp.path(), "init");
    let bin = env!("CARGO_BIN_EXE_kiss");
    let out = std::process::Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", ".", "--force"])
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "failing fixture must exit 1, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("FAIL:"),
        "streaming FAIL: line must remain: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line == "FAIL test_lib.py::test_f"),
        "recap must include colon-free FAIL selector, stdout={stdout}"
    );
    assert!(
        stdout.contains(" failed ·"),
        "recap must include failed count: {stdout}"
    );
}

#[test]
fn kiss_test_force_explicit_python_target_stays_selective_on_dry_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    std::fs::write(
        tmp.path().join("test_other.py"),
        "def test_other():\n    assert True\n",
    )
    .unwrap();
    commit_all(tmp.path(), "init");
    let out = kiss_test_dry_run(
        tmp.path(),
        &[
            "test",
            "test_lib.py::test_f",
            "--force",
            "--dry-run",
            "--lang",
            "python",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "forced explicit python dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("test_lib.py::test_f"),
        "expected selected selector in dry-run, got {stdout}"
    );
    assert!(
        !stdout.contains("test_other.py::test_other"),
        "forced explicit target must not widen to sibling tests, got {stdout}"
    );
    assert!(
        !stdout.contains("PYTHON COVERAGE POPULATION"),
        "forced explicit target must not print population marker, got {stdout}"
    );
}

#[test]
fn kiss_test_force_explicit_rust_target_stays_selective_on_dry_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn gets_value() {\n\
        assert_eq!(super::value(), 1);\n\
    }\n\
    #[test]\n\
    fn other() {\n\
        assert_eq!(super::value(), 1);\n\
    }\n\
}\n",
    )
    .unwrap();
    crate::common::generate_lockfile(tmp.path());
    commit_all(tmp.path(), "init");
    let out = kiss_test_dry_run(
        tmp.path(),
        &[
            "test",
            "src/lib.rs::gets_value",
            "--force",
            "--dry-run",
            "--lang",
            "rust",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "forced explicit rust dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("gets_value"),
        "expected selected selector in dry-run, got {stdout}"
    );
    assert!(
        !stdout.contains("::other") && !stdout.contains(" tests::other"),
        "forced explicit target must not widen to sibling tests, got {stdout}"
    );
    assert!(
        !stdout.contains("RUST COVERAGE POPULATION"),
        "forced explicit target must not print population marker, got {stdout}"
    );
}

#[test]
fn kiss_test_force_dot_python_dry_run_prints_population_marker() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    commit_all(tmp.path(), "init");
    let out = kiss_test_dry_run(
        tmp.path(),
        &["test", ".", "--force", "--dry-run", "--lang", "python"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "forced . python dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("PYTHON COVERAGE POPULATION"),
        "kiss test . --force must keep complete-population dry-run, got {stdout}"
    );
}

#[test]
fn kiss_test_rust_failure_prints_canonical_failed_recap_line() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn gets_value() {\n\
        assert_eq!(super::value(), 2);\n\
    }\n\
}\n",
    )
    .unwrap();
    crate::common::generate_lockfile(tmp.path());
    commit_all(tmp.path(), "init");
    let bin = env!("CARGO_BIN_EXE_kiss");
    let out = std::process::Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", ".", "--force", "--lang", "rust"])
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test rust fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "failing rust fixture must exit 1, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("FAIL:") || stdout.contains("FAIL ("),
        "streaming FAIL line must remain: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line == "FAIL src/lib.rs::gets_value"),
        "recap must use kiss-test PATH::symbol id, stdout={stdout}"
    );
    assert!(
        stdout.contains("FAIL: src/lib.rs::gets_value") || stdout.contains("FAIL ("),
        "streaming FAIL line must use PATH::symbol id: {stdout}"
    );
}
