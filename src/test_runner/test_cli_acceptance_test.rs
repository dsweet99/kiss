use crate::bin_cli::args::{
    Cli, Commands, TestInvocation, parse_test_invocation, validate_test_branch_options,
};
use crate::test_runner::test_mode_fixtures::warm_committed_rust_demo;
use clap::Parser;
use std::path::PathBuf;
use std::process::Command;

fn kiss_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/kiss")
}

#[test]
fn kiss_test_dot_dry_run_smoke() {
    let tmp = tempfile::TempDir::new().unwrap();
    warm_committed_rust_demo(&tmp);
    let config = tmp.path().join("builtin.kissconfig");
    std::fs::write(&config, "[python]\n[rust]\n").unwrap();
    let output = Command::new(kiss_bin())
        .current_dir(tmp.path())
        .args([
            "--config",
            config.to_str().unwrap(),
            "test",
            ".",
            "--dry-run",
            "--lang",
            "rust",
        ])
        .output()
        .expect("spawn kiss");
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("validate-selection"),
        "removed mode must not appear: {stderr}"
    );
}

#[test]
fn kiss_test_rejects_removed_cov_and_validate_selection() {
    for mode in ["cov", "validate-selection"] {
        let output = Command::new(kiss_bin())
            .args(["test", mode])
            .output()
            .expect("spawn kiss");
        assert_eq!(output.status.code(), Some(2), "mode={mode}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(mode) || stderr.contains("unknown test target"),
            "stderr={stderr}"
        );
    }
    let cov = Command::new(kiss_bin())
        .args(["test", "--help"])
        .output()
        .expect("spawn kiss test --help");
    assert!(cov.status.success());
}

#[test]
fn kiss_test_rejects_branch_options_outside_matching_modes() {
    let output = Command::new(kiss_bin())
        .args(["test", ".", "--main-branch", "main", "--dry-run"])
        .output()
        .expect("spawn kiss");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--main-branch"), "stderr={stderr}");

    let output = Command::new(kiss_bin())
        .args(["test", "commit", "--base-branch", "main", "--dry-run"])
        .output()
        .expect("spawn kiss");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn parse_rejects_framework_args_without_dashdash_as_mixed_reserved() {
    assert!(parse_test_invocation(&[".".into(), "-q".into()]).is_err());
    assert!(parse_test_invocation(&["all".into(), "-q".into()]).is_err());
    assert_eq!(
        parse_test_invocation(&["commit".into()]).unwrap(),
        TestInvocation::Commit
    );
    assert!(
        validate_test_branch_options(
            &TestInvocation::Targets(vec!["a.py".into()]),
            None,
            Some("x")
        )
        .is_err()
    );
}

#[test]
fn clap_keeps_trailing_extra_after_dashdash() {
    let cli = Cli::parse_from(["kiss", "test", ".", "--", "-q"]);
    match cli.command {
        Commands::Test {
            operands, extra, ..
        } => {
            assert_eq!(operands, vec![".".to_string()]);
            assert_eq!(extra, vec!["-q".to_string()]);
        }
        _ => panic!("expected Test"),
    }
}
