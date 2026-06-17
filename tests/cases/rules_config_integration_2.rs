use std::fs;
use std::process::Command;

use crate::support::kiss_test::kiss_command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    kiss_command()
}

#[test]
fn cli_rules_with_custom_config_file() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("custom.kissconfig");
    fs::write(&config_path, "[python]\nstatements_per_function = 42\n").unwrap();
    let output = kiss_binary()
        .arg("rules")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("42"),
        "Should reflect custom threshold. stdout: {stdout}"
    );
}

#[test]
fn cli_config_nonexistent_file_warns() {
    let output = kiss_binary()
        .arg("config")
        .arg("--config")
        .arg("/nonexistent/path/config")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning") || stderr.contains("Could not read"),
        "Should warn about missing file. stderr: {stderr}"
    );
}
