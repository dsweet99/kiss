//! Regression: runtime line-coverage violations must not read "100% covered".

use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn bug_check_all_never_claims_100_percent_on_unreferenced_unit() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    fs::write(repo.path().join("lib.py"), "def helper():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(
        repo.path(),
        &[("test_lib.py::test_helper", vec![("lib.py", vec![1])])],
    );

    let out = kiss_binary()
        .arg("cov")
        .arg("--all")
        .arg("--lang")
        .arg("python")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss cov --all should run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VIOLATION:test_coverage"),
        "expected coverage violations for unreferenced helpers:\n{stdout}"
    );
    assert!(
        !stdout.contains("100% covered"),
        "unreferenced unit lines must not claim 100% covered:\n{stdout}"
    );
}
