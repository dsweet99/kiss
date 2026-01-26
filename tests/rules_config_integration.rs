use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_rules_command_runs() {
    let output = kiss_binary().arg("rules").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "kiss rules should succeed. stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("DEFINITION:"), "Should output definitions. stdout: {stdout}");
    assert!(stdout.contains("RULE:"), "Should output rules. stdout: {stdout}");
}

#[test]
fn cli_rules_shows_both_languages_by_default() {
    let output = kiss_binary().arg("rules").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[Python]"), "Should show Python rules. stdout: {stdout}");
    assert!(stdout.contains("[Rust]"), "Should show Rust rules. stdout: {stdout}");
}

#[test]
fn cli_rules_with_defaults_flag() {
    let output = kiss_binary().arg("rules").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("DEFINITION:"), "Should output definitions with --defaults. stdout: {stdout}");
    assert!(stdout.contains("RULE:"), "Should output rules with --defaults. stdout: {stdout}");
    assert!(stdout.contains("35"), "Python defaults should have 35 statements. stdout: {stdout}");
    assert!(stdout.contains("25"), "Rust defaults should have 25 statements. stdout: {stdout}");
}

#[test]
fn cli_rules_filter_python_only() {
    let output = kiss_binary().arg("rules").arg("--lang").arg("python").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("[Python]"), "Should show Python rules. stdout: {stdout}");
    assert!(!stdout.contains("[Rust]"), "Should not show Rust rules. stdout: {stdout}");
}

#[test]
fn cli_rules_filter_rust_only() {
    let output = kiss_binary().arg("rules").arg("--lang").arg("rust").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("[Rust]"), "Should show Rust rules. stdout: {stdout}");
    assert!(!stdout.contains("[Python]"), "Should not show Python rules. stdout: {stdout}");
}

#[test]
fn cli_rules_shows_key_thresholds() {
    let output = kiss_binary().arg("rules").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("statements"), "Should mention statements. stdout: {stdout}");
    assert!(stdout.contains("methods"), "Should mention methods. stdout: {stdout}");
    assert!(stdout.contains("indentation"), "Should mention indentation. stdout: {stdout}");
}

#[test]
fn cli_config_command_runs() {
    let output = kiss_binary().arg("config").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "kiss config should succeed. stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("[python]") || stdout.contains("[rust]"), "Should output config sections. stdout: {stdout}");
}

#[test]
fn cli_config_shows_effective_configuration() {
    let output = kiss_binary().arg("config").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Effective configuration"), "Should show header. stdout: {stdout}");
    assert!(stdout.contains("[python]"), "Should show python section. stdout: {stdout}");
    assert!(stdout.contains("[rust]"), "Should show rust section. stdout: {stdout}");
    assert!(stdout.contains("[gate]"), "Should show gate section. stdout: {stdout}");
}

#[test]
fn cli_config_with_defaults_flag() {
    let output = kiss_binary().arg("config").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("built-in defaults"), "Should indicate defaults source. stdout: {stdout}");
    assert!(stdout.contains("statements_per_function = 35"), "Python statements should be 35. stdout: {stdout}");
    assert!(stdout.contains("statements_per_function = 25"), "Rust statements should be 25. stdout: {stdout}");
}

#[test]
fn cli_config_shows_gate_settings() {
    let output = kiss_binary().arg("config").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test_coverage_threshold = 90"), "Should show default coverage. stdout: {stdout}");
    assert!(stdout.contains("min_similarity = 0.70"), "Should show default similarity. stdout: {stdout}");
}

#[test]
fn cli_config_with_custom_file() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("custom.kissconfig");
    fs::write(&config_path, "[python]\nstatements_per_function = 99\n").unwrap();
    let output = kiss_binary().arg("config").arg("--config").arg(&config_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("99"), "Should reflect custom config value. stdout: {stdout}");
    assert!(stdout.contains(&config_path.to_string_lossy().to_string()) || stdout.contains("custom.kissconfig"), 
        "Should show config source. stdout: {stdout}");
}

#[test]
fn cli_config_shows_python_specific_settings() {
    let output = kiss_binary().arg("config").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("positional_args"), "Should show positional_args. stdout: {stdout}");
    assert!(stdout.contains("keyword_only_args"), "Should show keyword_only_args. stdout: {stdout}");
    assert!(stdout.contains("decorators_per_function"), "Should show decorators_per_function. stdout: {stdout}");
    assert!(stdout.contains("statements_per_try_block"), "Should show statements_per_try_block. stdout: {stdout}");
}

#[test]
fn cli_config_shows_rust_specific_settings() {
    let output = kiss_binary().arg("config").arg("--defaults").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("arguments = 8"), "Should show Rust arguments. stdout: {stdout}");
    assert!(stdout.contains("interface_types_per_file"), "Should show interface_types_per_file. stdout: {stdout}");
    assert!(stdout.contains("concrete_types_per_file"), "Should show concrete_types_per_file. stdout: {stdout}");
    assert!(stdout.contains("attributes_per_function"), "Should show attributes_per_function. stdout: {stdout}");
}

#[test]
fn cli_rules_with_custom_config_file() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("custom.kissconfig");
    fs::write(&config_path, "[python]\nstatements_per_function = 42\n").unwrap();
    let output = kiss_binary().arg("rules").arg("--config").arg(&config_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("42"), "Should reflect custom threshold. stdout: {stdout}");
}

#[test]
fn cli_config_nonexistent_file_warns() {
    let output = kiss_binary().arg("config").arg("--config").arg("/nonexistent/path/config").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Warning") || stderr.contains("Could not read"), "Should warn about missing file. stderr: {stderr}");
}

