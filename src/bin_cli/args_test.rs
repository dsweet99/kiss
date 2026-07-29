use super::*;
use clap::{CommandFactory, Parser};


impl Cli {
    fn witness() -> Self {
        Cli::parse_from(["kiss", "rules"])
    }
}

impl Commands {
    fn witness() -> Self {
        Commands::Rules
    }
}

#[test]
fn witness_cli_types() {
    let _ = Cli::witness();
    let _ = Commands::witness();
    assert!(parse_language("python").is_ok());
}

#[test]
fn cov_accepts_jobs_override() {
    let cli = Cli::parse_from(["kiss", "cov", "-j", "7"]);
    assert!(matches!(cli.command, Commands::Cov { jobs: Some(7), .. }));
}

#[test]
fn cov_jobs_defaults_to_none_for_config_num_jobs() {
    let cli = Cli::parse_from(["kiss", "cov"]);
    assert!(matches!(cli.command, Commands::Cov { jobs: None, .. }));
}

#[test]
fn cov_rejects_zero_jobs_override() {
    assert!(Cli::try_parse_from(["kiss", "cov", "-j", "0"]).is_err());
}

#[test]
fn test_accepts_jobs_override() {
    let cli = Cli::parse_from(["kiss", "test", "all", "-j", "7"]);
    assert!(matches!(cli.command, Commands::Test { jobs: Some(7), .. }));
}

#[test]
fn test_jobs_defaults_to_none_for_config_num_jobs() {
    let cli = Cli::parse_from(["kiss", "test", "all"]);
    assert!(matches!(cli.command, Commands::Test { jobs: None, .. }));
}

#[test]
fn test_rejects_zero_jobs_override() {
    assert!(Cli::try_parse_from(["kiss", "test", "all", "-j", "0"]).is_err());
}

#[test]
fn check_rejects_removed_coverage_flags() {
    assert!(Cli::try_parse_from(["kiss", "check", "--all"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "check", "-j", "2"]).is_err());
}

#[test]
fn test_invocation_parses_modes_all_and_targets() {
    assert_eq!(
        parse_test_invocation(&["all".into()]).unwrap(),
        TestInvocation::All
    );
    assert_eq!(
        parse_test_invocation(&["commit".into()]).unwrap(),
        TestInvocation::Commit
    );
    assert_eq!(
        parse_test_invocation(&["src/lib.rs".into(), "tests/test_x.py::test_y".into()])
            .unwrap(),
        TestInvocation::Targets(vec![
            "src/lib.rs".into(),
            "tests/test_x.py::test_y".into()
        ])
    );
    assert!(parse_test_invocation(&[]).is_err());
    assert!(parse_test_invocation(&["all".into(), "src/lib.rs".into()]).is_err());
    assert!(parse_test_invocation(&["cov".into()]).is_err());
    assert!(parse_test_invocation(&["validate-selection".into()]).is_err());
}

#[test]
fn test_branch_options_are_mode_specific() {
    assert!(
        validate_test_branch_options(&TestInvocation::Main, Some("main"), None).is_ok()
    );
    assert!(
        validate_test_branch_options(&TestInvocation::Base, None, Some("origin/main")).is_ok()
    );
    assert!(
        validate_test_branch_options(&TestInvocation::All, Some("main"), None).is_err()
    );
    assert!(
        validate_test_branch_options(&TestInvocation::Commit, None, Some("base")).is_err()
    );
    assert!(
        validate_test_branch_options(
            &TestInvocation::Targets(vec!["a.py".into()]),
            Some("main"),
            None
        )
        .is_err()
    );
}

#[test]
fn test_command_help_is_language_neutral_for_shared_options() {
    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("test")
        .expect("test subcommand exists")
        .render_long_help()
        .to_string();

    assert!(
        help.contains("Force selected tests to rerun instead of reusing test-runner caches")
    );
    assert!(help.contains("Maximum number of test jobs to run concurrently"));
    assert!(help.contains("all"));
    assert!(!help.contains("validate-selection"));
    assert!(!help.contains("Force Python tests"));
    assert!(!help.contains("Maximum number of Python test jobs"));
}

#[test]
fn test_cli_parses_targets_and_rejects_removed_modes() {
    let cli = Cli::parse_from([
        "kiss",
        "test",
        "src/lib.rs",
        "tests/test_x.py::test_y",
        "--dry-run",
    ]);
    match cli.command {
        Commands::Test { operands, dry_run, .. } => {
            assert_eq!(
                operands,
                vec!["src/lib.rs".to_string(), "tests/test_x.py::test_y".to_string()]
            );
            assert!(dry_run);
        }
        _ => panic!("expected Test"),
    }
    assert!(Cli::try_parse_from(["kiss", "test"]).is_err());
    let cli = Cli::parse_from(["kiss", "test", "cov"]);
    match cli.command {
        Commands::Test { operands, .. } => {
            assert!(parse_test_invocation(&operands).is_err());
        }
        _ => panic!("expected Test"),
    }
}

