use crate::common::seed_python_runtime_coverage;
use crate::support::git::{commit_all, init_git_repo};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn run_default_check_with_config(
    home: &std::path::Path,
    repo: &std::path::Path,
) -> std::process::Output {
    kiss_binary()
        .arg("check")
        .arg("--config")
        .arg(repo.join(".kissconfig"))
        .arg("--lang")
        .arg("python")
        .arg(repo)
        .env("HOME", home)
        .output()
        .unwrap()
}

// Cold/warm coverage-gate equality for seeded incomplete production coverage is
// covered in-process by
// `bin_cli::cov_cmd::tests::run_cov_command_warm_seed_still_emits_production_coverage_violations`.

#[test]
fn regression_cached_coverage_violations_do_not_leak_into_default_gate_mode() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_git_repo(repo.path());

    fs::write(
        repo.path().join(".kissconfig"),
        "[test]\ntest_coverage_threshold = 0\n[python]\n[rust]\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("default.py"),
        "def uncovered_function(x):\n    return x * 2\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_default.py::test_default",
            vec![("default.py", vec![])],
        )],
    );
    commit_all(repo.path(), "init");

    let cold = run_default_check_with_config(home.path(), repo.path());
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();
    assert_eq!(cold.status.code(), Some(0));
    assert!(!cold_stdout.contains("VIOLATION:test_coverage"));

    let all = kiss_binary()
        .current_dir(repo.path())
        .arg("test")
        .arg("--config")
        .arg(repo.path().join(".kissconfig"))
        .arg("--lang")
        .arg("python")
        .arg("--coverage-all")
        .arg(".")
        .env("HOME", home.path())
        .output()
        .unwrap();
    let all_stdout = String::from_utf8_lossy(&all.stdout).to_string();
    assert_eq!(all.status.code(), Some(1));
    assert!(all_stdout.contains("VIOLATION:test_coverage"));

    let warm_default = run_default_check_with_config(home.path(), repo.path());
    let warm_stdout = String::from_utf8_lossy(&warm_default.stdout).to_string();
    assert_eq!(warm_default.status.code(), cold.status.code());
    assert!(!warm_stdout.contains("VIOLATION:test_coverage"));
}

#[test]
fn kiss_check_ignores_seeded_below_threshold_runtime_coverage() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\nduplication_enabled = false\n[test]\ntest_coverage_threshold = 100\n[python]\n[rust]\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("default.py"),
        "def uncovered_function(x):\n    return x * 2\n",
    )
    .unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_default.py::test_default", vec![])]);

    let out = run_default_check_with_config(home.path(), repo.path());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(
        out.status.code(),
        Some(0),
        "static check must pass despite below-threshold seeded coverage.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("VIOLATION:test_coverage"),
        "check must not emit coverage gates/violations. stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("refreshing")
            && !stderr.contains("kiss test:")
            && !stderr.contains("PASSED:"),
        "check must not refresh or run the test population. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
}
