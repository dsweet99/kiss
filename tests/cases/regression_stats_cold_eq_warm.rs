use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn regression_stats_cold_and_warm_are_identical() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        repo.path().join("stats.py"),
        "def f(x):\n    return x\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("test_stats.py"),
        "def test_f():\n    assert f(1) == 1\n",
    )
    .unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 0\n",
    )
    .unwrap();

    let cold = kiss_binary()
        .arg("--defaults")
        .arg("stats")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    let warm = kiss_binary()
        .arg("--defaults")
        .arg("stats")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(
        {
            let cold_out = String::from_utf8_lossy(&cold.stdout);
            let warm_out = String::from_utf8_lossy(&warm.stdout);
            let mut left = cold_out
                .lines()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            let mut right = warm_out
                .lines()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            left.sort_unstable();
            right.sort_unstable();
            left == right
        },
        "sorted stats output should match on cold and warm runs"
    );
    assert_eq!(cold.status.code(), warm.status.code());
}
