use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn run_default_cov(home: &std::path::Path, repo: &std::path::Path) -> std::process::Output {
    kiss_binary()
        .arg("cov")
        .arg("--config")
        .arg(repo.join(".kissconfig"))
        .arg("--lang")
        .arg("python")
        .arg(repo)
        .env("HOME", home)
        .output()
        .unwrap()
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

#[test]
fn regression_check_default_warm_gate_matches_cold_and_warm_output() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 100\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("default.py"),
        "def uncovered_function(x):\n    return x * 2\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("test_default.py"),
        "def test_default():\n    assert True\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_default.py::test_default",
            vec![("test_default.py", vec![1, 2])],
        )],
    );

    let cold = run_default_cov(home.path(), repo.path());
    let warm = run_default_cov(home.path(), repo.path());

    assert_eq!(
        cold.status.code(),
        warm.status.code(),
        "exit status should match on cold and warm default runs"
    );
    assert_eq!(
        String::from_utf8_lossy(&cold.stdout),
        String::from_utf8_lossy(&warm.stdout),
        "default warm-hit output should match cold-hit output"
    );
    assert!(String::from_utf8_lossy(&cold.stdout).contains("GATE_FAILED:test_coverage:"));
    assert!(String::from_utf8_lossy(&warm.stdout).contains("GATE_FAILED:test_coverage:"));
}

#[test]
fn regression_cached_coverage_violations_do_not_leak_into_default_gate_mode() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 0\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("default.py"),
        "def uncovered_function(x):\n    return x * 2\n",
    )
    .unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_default.py::test_default", vec![])]);

    let cold = run_default_check_with_config(home.path(), repo.path());
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();
    assert_eq!(cold.status.code(), Some(0));
    assert!(!cold_stdout.contains("GATE_FAILED:test_coverage:"));
    assert!(!cold_stdout.contains("VIOLATION:test_coverage"));

    let all = kiss_binary()
        .arg("cov")
        .arg("--config")
        .arg(repo.path().join(".kissconfig"))
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let all_stdout = String::from_utf8_lossy(&all.stdout).to_string();
    assert_eq!(all.status.code(), Some(1));
    assert!(all_stdout.contains("VIOLATION:test_coverage"));

    let warm_default = run_default_check_with_config(home.path(), repo.path());
    let warm_stdout = String::from_utf8_lossy(&warm_default.stdout).to_string();
    assert_eq!(warm_default.status.code(), cold.status.code());
    assert!(!warm_stdout.contains("GATE_FAILED:test_coverage:"));
    assert!(!warm_stdout.contains("VIOLATION:test_coverage"));
}

#[test]
fn regression_default_gate_fail_still_reports_timing() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 100\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("default.py"),
        "def uncovered_function(x):\n    return x * 2\n",
    )
    .unwrap();
    seed_python_runtime_coverage(repo.path(), &[("test_default.py::test_default", vec![])]);

    let out = kiss_binary()
        .arg("cov")
        .arg("--config")
        .arg(repo.path().join(".kissconfig"))
        .arg("--timing")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("TIMING:coverage_snapshot_load_or_refresh_ms"));
    assert!(
        !stderr.contains("TIMING:parse")
            && !stderr.contains("TIMING:graph")
            && !stderr.contains("TIMING:phase"),
        "cov --timing must not emit static-analysis timing labels. stderr:\n{stderr}"
    );
}

#[test]
fn kiss_check_ignores_seeded_below_threshold_runtime_coverage() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 100\nduplication_enabled = false\norphan_module_enabled = false\n",
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
        !stdout.contains("GATE_FAILED:test_coverage")
            && !stdout.contains("VIOLATION:test_coverage"),
        "check must not emit coverage gates/violations. stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("refreshing")
            && !stderr.contains("kiss cov:")
            && !stderr.contains("PASSED:"),
        "check must not refresh or run the test population. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
}
