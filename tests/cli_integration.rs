//! CLI integration tests for the kiss binary
//!
//! These tests verify that the kiss binary runs correctly and produces expected output.

use std::process::Command;

/// Get the path to the kiss binary
fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_analyze_on_fake_code_runs_successfully() {
    let output = kiss_binary()
        .arg("tests/fake_code")
        .arg("--all") // bypass coverage gate for test
        .output()
        .expect("Failed to execute kiss");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should complete without panic (exit code 0 or produce output)
    assert!(
        output.status.success() || !stdout.is_empty(),
        "kiss should run successfully. stderr: {}",
        stderr
    );
}

#[test]
fn cli_analyze_reports_violations_on_god_class() {
    let output = kiss_binary()
        .arg("tests/fake_code/god_class.py")
        .arg("--all")
        .arg("--lang")
        .arg("python")
        .output()
        .expect("Failed to execute kiss");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // god_class.py should trigger violations
    assert!(
        stdout.contains("violation") || stdout.contains("methods"),
        "god_class.py should report violations. stdout: {}",
        stdout
    );
}

#[test]
fn cli_stats_command_runs() {
    let output = kiss_binary()
        .arg("stats")
        .arg("tests/fake_code")
        .output()
        .expect("Failed to execute kiss stats");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should produce stats output
    assert!(
        stdout.contains("stats") || stdout.contains("Python") || stdout.contains("files"),
        "kiss stats should produce output. stdout: {}",
        stdout
    );
}

#[test]
fn cli_with_lang_filter_python() {
    let output = kiss_binary()
        .arg("tests/fake_code")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .output()
        .expect("Failed to execute kiss");

    // Should run without error
    assert!(
        output.status.success(),
        "kiss --lang python should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_with_lang_filter_rust() {
    let output = kiss_binary()
        .arg("tests/fake_code")
        .arg("--lang")
        .arg("rust")
        .arg("--all")
        .output()
        .expect("Failed to execute kiss");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should report no Rust files found (fake_code only has Python)
    assert!(
        stdout.contains("No Rust files") || stdout.contains("No files"),
        "Should report no Rust files. stdout: {}",
        stdout
    );
}

#[test]
fn cli_help_flag_works() {
    let output = kiss_binary()
        .arg("--help")
        .output()
        .expect("Failed to execute kiss --help");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("kiss") || stdout.contains("Code-quality"));
}

#[test]
fn cli_version_flag_works() {
    let output = kiss_binary()
        .arg("--version")
        .output()
        .expect("Failed to execute kiss --version");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("kiss") || stdout.contains("0."));
}

#[test]
fn cli_invalid_lang_reports_error() {
    let output = kiss_binary()
        .arg("--lang")
        .arg("invalid_language")
        .arg(".")
        .output()
        .expect("Failed to execute kiss");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains("Unknown language") || stderr.contains("error"),
        "Should report unknown language error. stderr: {}",
        stderr
    );
}

#[test]
fn cli_on_empty_directory() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let output = kiss_binary()
        .arg(tmp.path())
        .arg("--all")
        .output()
        .expect("Failed to execute kiss");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should report no files found
    assert!(
        stdout.contains("No files") || stdout.contains("No Python") || stdout.contains("No Rust"),
        "Should report no files. stdout: {}",
        stdout
    );
}

#[test]
fn cli_mimic_command_runs() {
    let output = kiss_binary()
        .arg("mimic")
        .arg("tests/fake_code")
        .output()
        .expect("Failed to execute kiss mimic");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should produce TOML config output
    assert!(
        stdout.contains("[python]") || stdout.contains("Generated"),
        "kiss mimic should produce config. stdout: {}",
        stdout
    );
}

