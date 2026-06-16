use clap::Parser;

use crate::bin_cli::args::{Cli, Commands};

#[test]
fn cli_parses_j_default_and_override() {
    let test = Cli::try_parse_from(["kiss", "test", "commit"]).unwrap();
    match test.command {
        Commands::Test { jobs, .. } => assert_eq!(jobs, None),
        _ => panic!("expected test command"),
    }

    let test = Cli::try_parse_from(["kiss", "test", "commit", "-j", "4"]).unwrap();
    match test.command {
        Commands::Test { jobs, .. } => assert_eq!(jobs, Some(4)),
        _ => panic!("expected test command"),
    }

    let check = Cli::try_parse_from(["kiss", "check", "-j", "2"]).unwrap();
    match check.command {
        Commands::Check { jobs, .. } => assert_eq!(jobs, Some(2)),
        _ => panic!("expected check command"),
    }
}

#[test]
fn default_parallelism_is_at_least_one() {
    assert!(pyfork::default_parallelism() >= 1);
}
