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
fn defaults_flag_is_removed() {
    assert!(Cli::try_parse_from(["kiss", "--defaults", "rules"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "rules", "--defaults"]).is_err());
}

#[test]
fn config_flag_parses_before_and_after_subcommand() {
    let before = Cli::parse_from(["kiss", "--config", "custom.toml", "rules"]);
    assert_eq!(
        before.config.as_deref(),
        Some(std::path::Path::new("custom.toml"))
    );
    let after = Cli::parse_from(["kiss", "rules", "--config", "other.toml"]);
    assert_eq!(
        after.config.as_deref(),
        Some(std::path::Path::new("other.toml"))
    );
}

#[test]
fn cov_subcommand_parses_as_coverage() {
    assert!(Cli::try_parse_from(["kiss", "cov"]).is_err());
    assert!(Cli::command().find_subcommand("cov").is_none());
    let help = Cli::command().render_long_help().to_string();
    assert!(help.contains("kiss test"), "{help}");
    assert!(
        !help
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w.eq_ignore_ascii_case("cov")),
        "top-level help must not mention the token cov\n{help}"
    );
    assert!(Cli::try_parse_from(["kiss", "__coverage"]).is_err());
    assert!(Cli::command().find_subcommand("__coverage").is_none());
}

#[test]
fn readme_does_not_mention_cov_token() {
    let readme = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"));
    assert!(
        !readme
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w.eq_ignore_ascii_case("cov")),
        "README.md must not mention the token cov"
    );
}

#[test]
fn config_subcommand_is_removed() {
    assert!(Cli::try_parse_from(["kiss", "config"]).is_err());
    assert!(Cli::command().find_subcommand("config").is_none());
}

#[test]
fn init_mimic_and_clamp_commands_are_removed() {
    assert!(Cli::try_parse_from(["kiss", "init"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "mimic", "."]).is_err());
    assert!(Cli::try_parse_from(["kiss", "clamp"]).is_err());
}

#[test]
fn help_subcommand_is_removed() {
    assert!(Cli::try_parse_from(["kiss", "help"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "help", "check"]).is_err());
    assert!(Cli::command().find_subcommand("help").is_none());
    let help = Cli::command().render_long_help().to_string();
    assert!(help.contains("--help"));
    assert!(!help.contains("Print this message or the help of the given subcommand"));
}

#[test]
fn test_accepts_coverage_all_flag() {
    let cli = Cli::parse_from(["kiss", "test", ".", "--coverage-all"]);
    assert!(matches!(
        cli.command,
        Commands::Test {
            coverage_all: true,
            ..
        }
    ));
}

#[test]
fn test_accepts_jobs_override() {
    let cli = Cli::parse_from(["kiss", "test", ".", "-j", "7"]);
    assert!(matches!(cli.command, Commands::Test { jobs: Some(7), .. }));
}

#[test]
fn test_jobs_defaults_to_none_for_config_num_jobs() {
    let cli = Cli::parse_from(["kiss", "test", "."]);
    assert!(matches!(cli.command, Commands::Test { jobs: None, .. }));
}

#[test]
fn test_rejects_zero_jobs_override() {
    assert!(Cli::try_parse_from(["kiss", "test", ".", "-j", "0"]).is_err());
}

#[test]
fn check_rejects_removed_coverage_flags() {
    assert!(Cli::try_parse_from(["kiss", "check", "--all"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "check", "-j", "2"]).is_err());
}

#[test]
fn test_retry_bad_parses_and_removed_force_flags_are_rejected() {
    let cli = Cli::parse_from(["kiss", "test", ".", "--retry-bad"]);
    assert!(matches!(
        cli.command,
        Commands::Test {
            retry_bad: true,
            ..
        }
    ));
    assert!(Cli::try_parse_from(["kiss", "test", ".", "--force"]).is_err());
    assert!(Cli::try_parse_from(["kiss", "test", ".", "--force-bad"]).is_err());
}

#[test]
fn test_invocation_parses_modes_dot_all_and_targets() {
    assert_eq!(
        parse_test_invocation(&[".".into()]).unwrap(),
        TestInvocation::All
    );
    assert_eq!(
        parse_test_invocation(&["./".into()]).unwrap(),
        TestInvocation::All
    );
    let all_err = parse_test_invocation(&["all".into()]).unwrap_err();
    assert!(all_err.contains("kiss test ."), "{all_err}");
    assert!(parse_test_invocation(&["./all".into()]).is_ok());
    assert_eq!(
        parse_test_invocation(&["commit".into()]).unwrap(),
        TestInvocation::Commit
    );
    assert_eq!(
        parse_test_invocation(&["src/lib.rs".into(), "tests/test_x.py::test_y".into()]).unwrap(),
        TestInvocation::Targets(vec!["src/lib.rs".into(), "tests/test_x.py::test_y".into()])
    );
    assert_eq!(
        parse_test_invocation(&["src".into(), "crates/foo".into()]).unwrap(),
        TestInvocation::Targets(vec!["src".into(), "crates/foo".into()])
    );
    assert_eq!(parse_test_invocation(&[]).unwrap(), TestInvocation::All);
    assert!(parse_test_invocation(&[".".into(), "src/lib.rs".into()]).is_err());
    assert!(parse_test_invocation(&["src::symbol".into()]).is_err());
    assert!(parse_test_invocation(&["cov".into()]).is_err());
    assert!(parse_test_invocation(&["validate-selection".into()]).is_err());
}

#[test]
fn test_branch_options_are_mode_specific() {
    assert!(validate_test_branch_options(&TestInvocation::Main, Some("main"), None).is_ok());
    assert!(validate_test_branch_options(&TestInvocation::Base, None, Some("origin/main")).is_ok());
    assert!(validate_test_branch_options(&TestInvocation::All, Some("main"), None).is_err());
    assert!(validate_test_branch_options(&TestInvocation::Commit, None, Some("base")).is_err());
    assert!(validate_test_branch_options(
        &TestInvocation::Targets(vec!["a.py".into()]),
        Some("main"),
        None
    )
    .is_err());
}

#[test]
fn test_command_help_is_language_neutral_for_shared_options() {
    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("test")
        .expect("test subcommand exists")
        .render_long_help()
        .to_string();

    assert!(help.contains("--retry-bad"));
    assert!(!help.contains("--force"));
    assert!(help
        .contains("Rerun FAIL and TIMEOUT tests in the TARGET subset"));
    assert!(help.contains("Maximum number of test jobs to run concurrently"));
    assert!(help.contains("commit, base, main, ., or PATH / PATH::symbol / directory"));
    assert!(help.contains("[TARGET]"));
    assert!(!help.contains("commit|base|main|.|TARGET"));
    assert!(!help.contains("all|TARGET"));
    assert!(!help.contains("validate-selection"));
    assert!(!help.contains("Force Python tests"));
    assert!(!help.contains("Maximum number of Python test jobs"));
    assert!(help.contains("Show tests that would run without executing them"));
    assert!(help.contains("Rerun tests when sources change"));
}

#[test]
fn top_level_help_describes_commands_and_global_flags() {
    let help = Cli::command().render_long_help().to_string();
    let first_line = help.lines().next().expect("help has a first line");
    assert_eq!(first_line, "Global code feedback for LLM coding agents");
    assert!(!help.contains(" (Python and Rust)"));
    assert!(help.contains("Path to custom config file (default: .kissconfig)"));
    assert!(help.contains("Filter by language: python (py) or rust (rs)"));
    assert!(!help.contains("Use built-in defaults, ignoring config files"));
    assert!(!help.contains("--defaults"));
    assert!(help.contains("Run static complexity, graph, duplicate, comment, and doc checks"));
    assert!(help.contains("Show metric statistics for the codebase"));
    assert!(!help.contains("Generate .kissconfig thresholds from an existing codebase"));
    assert!(!help.contains("Generate .kissconfig from the current directory"));
    assert!(!help.contains("Write a default .kissconfig"));
    assert!(Cli::command().find_subcommand("init").is_none());
    assert!(Cli::command().find_subcommand("mimic").is_none());
    assert!(Cli::command().find_subcommand("clamp").is_none());
    assert!(help.contains("Detect duplicate code blocks"));
    assert!(help.contains("Display all available rules and their current thresholds"));
    assert!(!help.contains("Show effective configuration (merged from all sources)"));
    assert!(Cli::command().find_subcommand("config").is_none());
    assert!(Cli::command().find_subcommand("help").is_none());
    assert!(help.contains("Write a dependency graph"));
    assert!(!help.contains("Output markdown path"));
    assert!(help.contains("Run covering tests and enforce coverage and time gates"));
    assert!(!help.contains("Coverage-only evaluation (prefer kiss test for the full path)"));
    assert!(Cli::command().find_subcommand("__coverage").is_none());
    assert!(!help.contains("Coverage is enforced by kiss test, not a standalone command"));
    assert!(
        !help.lines().any(|line| {
            let name = line.split_whitespace().next();
            name == Some("cov")
        }),
        "top-level help must not list cov\n{help}"
    );
    assert!(help.contains("Rename or move a Python or Rust symbol (beta)"));
    assert!(help.contains("Usage:"));
    assert!(help.contains("Examples:"));
    assert!(help.contains("kiss check"));
    assert!(!help.contains("kiss clamp"));
    assert!(!help.contains("kiss mimic"));
    assert!(!help.contains("kiss init"));
    assert!(!help.contains("EXAMPLES:"));
}

#[test]
fn check_and_cov_help_describe_options() {
    let mut command = Cli::command();
    let check = command
        .find_subcommand_mut("check")
        .expect("check subcommand exists")
        .render_long_help()
        .to_string();
    assert!(check.contains("Codebase root, optionally followed by focus paths"));
    assert!(check.contains("Path prefix to exclude"));
    assert!(check.contains("Print analysis stage timings"));
    assert!(check.contains("If .kissconfig is missing, writes one from current-codebase maxima."));
    assert!(check.contains("Usage:"));
    assert!(Cli::command().find_subcommand("cov").is_none());
    let mut command = Cli::command();
    let viz = command
        .find_subcommand_mut("viz")
        .expect("viz subcommand exists")
        .render_long_help()
        .to_string();
    assert!(viz.contains("Output file (.md, .mmd/.mermaid, or .dot)"));
    assert!(!viz.contains("Output markdown path"));
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
        Commands::Test {
            operands, dry_run, ..
        } => {
            assert_eq!(
                operands,
                vec![
                    "src/lib.rs".to_string(),
                    "tests/test_x.py::test_y".to_string()
                ]
            );
            assert!(dry_run);
        }
        _ => panic!("expected Test"),
    }
    let bare = Cli::try_parse_from(["kiss", "test"]).unwrap();
    match bare.command {
        Commands::Test { operands, .. } => {
            assert_eq!(operands, vec![".".to_string()]);
            assert_eq!(
                parse_test_invocation(&operands).unwrap(),
                TestInvocation::All
            );
        }
        _ => panic!("expected Test"),
    }
    let cli = Cli::parse_from(["kiss", "test", "cov"]);
    match cli.command {
        Commands::Test { operands, .. } => {
            assert!(parse_test_invocation(&operands).is_err());
        }
        _ => panic!("expected Test"),
    }
}

#[test]
fn test_watch_flag_parses() {
    let cli = Cli::try_parse_from(["kiss", "test", "--watch", "commit"]).unwrap();
    match cli.command {
        Commands::Test {
            watch: true,
            operands,
            ..
        } => assert_eq!(operands, vec!["commit".to_string()]),
        _ => panic!("expected watch"),
    }
}

#[test]
fn test_watch_bg_flag_is_rejected_by_clap() {
    let err = Cli::try_parse_from(["kiss", "test", "--watch-bg", "commit"]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unexpected argument") || msg.contains("--watch-bg"),
        "msg={msg}"
    );
}
