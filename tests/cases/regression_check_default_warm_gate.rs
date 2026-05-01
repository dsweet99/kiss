use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
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

    let cold = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    let warm = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

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
