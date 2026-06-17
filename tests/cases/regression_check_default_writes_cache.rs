use crate::common::list_full_check_cache_files;
use crate::support::kiss_test::kiss_command;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    kiss_command()
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

#[test]
fn regression_check_default_writes_cache_for_coverage_gate_failure() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let src = repo.path().join("uncovered.py");

    fs::write(&src, "def uncovered_function(x):\n    return x + 1\n").unwrap();

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

    assert!(
        !cold.status.success(),
        "precondition: uncovered source should fail the default coverage gate.\nstdout:\n{cold_stdout}"
    );
    assert!(
        cold_stdout.contains("GATE_FAILED:test_coverage"),
        "expected coverage gate failure. stdout:\n{cold_stdout}"
    );
    assert!(
        !list_full_check_cache_files(home.path()).is_empty(),
        "failing coverage-gate check should still write a replayable full-check cache.\nstdout:\n{cold_stdout}"
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

#[test]
fn regression_check_default_writes_cache_for_rslip_refresh_failure() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::create_dir(repo.path().join("tests")).unwrap();
    fs::write(
        repo.path().join("app.py"),
        "def app_value():\n    return 3\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("tests").join("test_broken.py"),
        "import missing_dependency_for_rslip_test\n\n\
         def test_app_value():\n\
             assert False\n",
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
    let cold_stdout = String::from_utf8_lossy(&cold.stdout).to_string();

    assert!(
        !cold.status.success(),
        "precondition: pytest collection failure should fail the default coverage gate.\nstdout:\n{cold_stdout}"
    );
    assert!(
        cold_stdout.contains("rslip_refresh_failed"),
        "expected fail-closed rslip coverage output. stdout:\n{cold_stdout}"
    );
    assert!(
        !list_full_check_cache_files(home.path()).is_empty(),
        "rslip refresh failures should still write a replayable full-check cache.\nstdout:\n{cold_stdout}"
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
