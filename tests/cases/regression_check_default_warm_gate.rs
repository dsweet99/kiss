use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn run_default_check(home: &std::path::Path, repo: &std::path::Path) -> std::process::Output {
    kiss_binary()
        .arg("--defaults")
        .arg("check")
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

    let cold = run_default_check(home.path(), repo.path());
    let warm = run_default_check(home.path(), repo.path());

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
    assert!(
        String::from_utf8_lossy(&cold.stdout).contains("GATE_FAILED:test_coverage:")
    );
    assert!(
        String::from_utf8_lossy(&warm.stdout).contains("GATE_FAILED:test_coverage:")
    );
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

    let cold = run_default_check_with_config(home.path(), repo.path());
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();
    assert_eq!(cold.status.code(), Some(0));
    assert!(!cold_stdout.contains("GATE_FAILED:test_coverage:"));
    assert!(!cold_stdout.contains("VIOLATION:test_coverage"));

    let all = kiss_binary()
        .arg("check")
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

    let out = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--timing")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("[TIMING]"));
    assert!(stderr.contains("py: parse="));
    assert!(stderr.contains("analyze="));
}
