use std::fs;
use std::process::Command;
use kiss::cli_output::VIOLATIONS_FIX_HINT;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_check_default_gate_emits_hint_on_coverage_failure() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(tmp.path().join("orphan.py"), "def orphan():\n    pass\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 90\n",
    )
    .unwrap();
    let output = kiss_binary()
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .arg("check")
        .arg(".")
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
