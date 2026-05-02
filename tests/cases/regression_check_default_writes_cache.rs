use crate::common::list_full_check_cache_files;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn regression_check_default_writes_cache_and_replays() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let src = repo.path().join("default.py");
    let test = repo.path().join("test_default.py");

    fs::write(&src, "def covered_function(x):\n    return x * 2\n").unwrap();
    fs::write(&test, "from default import covered_function\n\ndef test_covered_function():\n    assert covered_function(2) == 4\n").unwrap();
    let cold = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();
    let cache_files = list_full_check_cache_files(home.path());
    assert!(
        !cache_files.is_empty(),
        "expected full-check cache file under HOME. stdout:\n{cold_stdout}"
    );

    let warm = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    let warm_stdout = String::from_utf8_lossy(&warm.stdout).to_string();
    assert_eq!(cold.status.code(), warm.status.code());
    assert_eq!(cold_stdout, warm_stdout);
}
