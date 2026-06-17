use clap::Parser;

use crate::bin_cli::args::{Cli, Commands};

#[test]
fn cli_parses_j_default_and_override() {
    let test = Cli::try_parse_from(["kiss", "test", "commit"]).unwrap();
    assert!(matches!(test.command, Commands::Test { jobs: None, .. }));

    let test = Cli::try_parse_from(["kiss", "test", "commit", "-j", "4"]).unwrap();
    assert!(matches!(test.command, Commands::Test { jobs: Some(4), .. }));

    let test_alias = Cli::try_parse_from(["kiss", "t", "base", "-j", "3"]).unwrap();
    assert!(matches!(
        test_alias.command,
        Commands::Test { jobs: Some(3), .. }
    ));

    let check = Cli::try_parse_from(["kiss", "check", "-j", "2"]).unwrap();
    assert!(matches!(
        check.command,
        Commands::Check { jobs: Some(2), .. }
    ));

    let lang = Cli::try_parse_from(["kiss", "--lang", "rs", "check", "-j", "1"]).unwrap();
    assert_eq!(lang.lang, Some(kiss::Language::Rust));
    assert!(matches!(
        lang.command,
        Commands::Check { jobs: Some(1), .. }
    ));
}

#[test]
fn default_parallelism_is_at_least_one() {
    assert!(pyfork::default_parallelism() >= 1);
}
