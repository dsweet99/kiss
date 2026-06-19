use crate::support::kiss_test::kiss_command;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    kiss_command()
}

#[test]
fn cli_check_default_gate_emits_only_violation_lines_on_coverage_failure() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("orphan.py"), "def orphan():\n    pass\n").unwrap();
    let output = kiss_binary()
        .arg("check")
        .arg("--defaults")
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VIOLATION:test_coverage:"),
        "expected coverage gate failure. stdout: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .all(|line| line.starts_with("VIOLATION:test_coverage:")),
        "coverage gate failure should emit only violation lines. stdout: {stdout}"
    );
}
