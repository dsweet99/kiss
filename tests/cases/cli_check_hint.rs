use crate::support::kiss_test::kiss_command;
use kiss::cli_output::VIOLATIONS_FIX_HINT;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    kiss_command()
}

#[test]
fn cli_check_default_gate_emits_hint_on_coverage_failure() {
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
        stdout.contains("GATE_FAILED:test_coverage:"),
        "expected coverage gate failure. stdout: {stdout}"
    );
    assert!(
        stdout.contains(VIOLATIONS_FIX_HINT),
        "coverage gate failure should include fix hint. stdout: {stdout}"
    );
}
