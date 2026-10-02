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

fn assert_dry_run_deferred_preview(mode: &str, out: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{mode}: kiss test --dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("kiss test: plan complete=false deferred=true"),
        "{mode}: expected deferred TargetPlanPreview, got {stdout}"
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
    assert_dry_run_deferred_preview("commit", &out);
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
    assert_dry_run_deferred_preview("base", &base_out);
    let main_out = kiss_test_dry_run(
        tmp.path(),
        &["test", "main", "--main-branch", "main", "--dry-run"],
    );
    assert_dry_run_deferred_preview("main", &main_out);
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
    crate::common::seed_python_runtime_coverage(
        tmp.path(),
        &[("test_lib.py::test_f", vec![("lib.py", vec![1, 2])])],
    );
    commit_all(tmp.path(), "init");
    let bin = env!("CARGO_BIN_EXE_kiss");
    let out = std::process::Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", "--lang", "python", "."])
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "kiss test should pass, stderr={stderr}, stdout={stdout}"
    );
    let recap = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with("✓ ") || line.starts_with("✗ "))
        .unwrap_or("");
    assert!(
        recap.starts_with("✓ ")
            && recap.contains(" passed · ")
            && recap.contains(" failed · ")
            && recap.contains(" timed out"),
        "seeded run must include official pass recap, recap={recap}, stdout={stdout}"
    );
    assert!(
        stdout.contains("PASS:")
            || stdout.contains("PASS (cached):")
            || stdout.contains("PASS "),
        "streaming PASS lines must remain: {stdout}"
    );
}

#[test]
fn kiss_test_python_failure_prints_failed_recap_line() {
    let repo = crate::common::persistent_python_failure_repo();
    let bin = env!("CARGO_BIN_EXE_kiss");
    let out = std::process::Command::new(bin)
        .current_dir(&repo)
        .args(["test", "test_lib.py::test_f", "--lang", "python"])
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
        stdout.contains("FAIL:") || stdout.contains("FAIL test_lib.py::test_f"),
        "streaming FAIL line must remain: {stdout}"
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
            "--dry-run",
            "--lang",
            "python",
        ],
    );
    assert_dry_run_prints_selector("explicit python", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
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
    let out = kiss_test_dry_run(tmp.path(), &["test", ".", "--dry-run", "--lang", "python"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "forced . python dry-run should exit 0, stderr={stderr}, stdout={stdout}"
    );
    assert!(
        stdout.contains("kiss test: plan complete=false deferred=true"),
        "kiss test . dry-run must render a deferred TargetPlanPreview, got {stdout}"
    );
    assert!(
        !stdout.contains("PYTHON COVERAGE POPULATION"),
        "deferred dry-run must not collect a coverage population, got {stdout}"
    );
}
