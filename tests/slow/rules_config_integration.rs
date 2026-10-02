use std::{fs, process::Command};
use tempfile::TempDir;
fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}
#[test]
fn cli_rules_command_runs() {
    let output = kiss_binary().arg("rules").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "kiss rules should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("DEFINITION:"),
        "Should output definitions. stdout: {stdout}"
    );
    assert!(
        stdout.contains("RULE: [global]") && stdout.contains("RULE: [test]"),
        "Should output global and test rules. stdout: {stdout}"
    );
}
#[test]
fn cli_rules_shows_both_languages_by_default() {
    let output = kiss_binary().arg("rules").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[Python]"),
        "Should show Python rules. stdout: {stdout}"
    );
    assert!(
        stdout.contains("[Rust]"),
        "Should show Rust rules. stdout: {stdout}"
    );
}
#[test]
fn cli_rules_with_defaults_flag() {
    let tmp = TempDir::new().unwrap();
    let config = crate::common::write_builtin_language_config(tmp.path());
    let output = kiss_binary()
        .arg("rules")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("DEFINITION:"),
        "Should output definitions with --config builtin. stdout: {stdout}"
    );
    assert!(
        stdout.contains("RULE:"),
        "Should output rules with --config builtin. stdout: {stdout}"
    );
    assert!(
        stdout.contains("35"),
        "Python defaults should have 35 statements. stdout: {stdout}"
    );
    assert!(
        stdout.contains("35"),
        "Rust defaults should have 35 statements. stdout: {stdout}"
    );
}

#[test]
fn cli_rules_filter_python_only() {
    let output = kiss_binary()
        .arg("rules")
        .arg("--lang")
        .arg("python")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("[Python]") && stdout.contains("RULE: [global]"),
        "Should show Python and global rules. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("[Rust]"),
        "Should not show Rust rules. stdout: {stdout}"
    );
}

#[test]
fn cli_rules_filter_rust_only() {
    let output = kiss_binary()
        .arg("rules")
        .arg("--lang")
        .arg("rust")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("[Rust]"),
        "Should show Rust rules. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("[Python]"),
        "Should not show Python rules. stdout: {stdout}"
    );
}

#[test]
fn cli_rules_shows_key_thresholds() {
    let tmp = TempDir::new().unwrap();
    let config = crate::common::write_builtin_language_config(tmp.path());
    let output = kiss_binary()
        .arg("rules")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("statements"),
        "Should mention statements. stdout: {stdout}"
    );
    assert!(
        stdout.contains("methods"),
        "Should mention methods. stdout: {stdout}"
    );
    assert!(
        stdout.contains("indentation"),
        "Should mention indentation. stdout: {stdout}"
    );
}

#[test]
fn cli_rules_ignores_home_kissconfig_without_local() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    let home = tmp.path().join("home");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&home).unwrap();
    fs::write(repo.join("sample.py"), "def foo():\n    return 1\n").unwrap();
    fs::write(
        home.join(".kissconfig"),
        "[python]\nstatements_per_function = 99\n",
    )
    .unwrap();

    let output = kiss_binary()
        .current_dir(&repo)
        .env("HOME", &home)
        .arg("rules")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "rules should succeed. stderr: {stderr}"
    );
    assert!(
        !stdout.contains("statements_per_function <= 99"),
        "Home .kissconfig must not be merged. stdout: {stdout}"
    );
}

#[test]
fn cli_missing_local_kissconfig_is_created_by_clamp() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    fs::write(repo.join("sample.py"), "def foo():\n    return 1\n").unwrap();

    let output = kiss_binary()
        .current_dir(&repo)
        .arg("rules")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "rules should succeed. stderr: {stderr}"
    );
    assert!(
        repo.join(".kissconfig").exists(),
        "kiss should clamp-write ./.kissconfig when missing"
    );
}

#[test]
fn cli_rules_with_custom_file_and_local_layers_merges() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    let home = tmp.path().join("home");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&home).unwrap();

    fs::write(
        repo.join(".kissconfig"),
        "[python]\nstatements_per_function = 100\n",
    )
    .unwrap();
    fs::write(home.join(".kissconfig"), "[shared]\nlines_per_file = 111\n").unwrap();

    let config_path = repo.join("custom.kissconfig");
    fs::write(&config_path, "[python]\npositional_args = 1\n").unwrap();

    let output = kiss_binary()
        .current_dir(&repo)
        .env("HOME", &home)
        .arg("rules")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(
        !stdout.contains("statements_per_function <= 100"),
        "--config must replace local .kissconfig. stdout: {stdout}"
    );
    assert!(
        stdout.contains("statements_per_function <= 35"),
        "Unset keys should keep built-in defaults. stdout: {stdout}"
    );
    assert!(
        !stdout.contains("lines_per_file <= 111"),
        "Home .kissconfig should not be merged. stdout: {stdout}"
    );
    assert!(
        stdout.contains("positional_args <= 1"),
        "Explicit --config should apply. stdout: {stdout}"
    );
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
fn cli_rules_nonexistent_file_warns() {
    let output = kiss_binary()
        .arg("rules")
        .arg("--config")
        .arg("/nonexistent/path/config")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "missing --config should warn and continue. stderr: {stderr}"
    );
    assert!(
        stderr.contains("Warning") || stderr.contains("Could not read"),
        "Should warn about missing file. stderr: {stderr}"
    );
}
