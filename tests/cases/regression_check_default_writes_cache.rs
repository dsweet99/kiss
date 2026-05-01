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
    let xdg_cache_home = home.path().join(".cache-home");

    let cold = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", &xdg_cache_home)
        .output()
        .unwrap();
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();

    let warm = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", &xdg_cache_home)
        .output()
        .unwrap();

    let warm_stdout = String::from_utf8_lossy(&warm.stdout).to_string();
    assert_eq!(cold.status.code(), warm.status.code());
    assert_eq!(cold_stdout, warm_stdout);
}
