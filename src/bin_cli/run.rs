use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::config_session::{
    ensure_default_config_exists, load_configs, load_gate_config, load_test_section_config,
    run_init_command,
};
use crate::bin_cli::dispatch::dispatch;
use clap::Parser;

pub fn run_cli_entrypoint() -> i32 {
    run_with_cli(parse_cli())
}

#[cfg(test)]
pub use run_cli_entrypoint as run;

pub(crate) fn run_with_cli(cli: Cli) -> i32 {
    if let Commands::Init { repo_path } = &cli.command {
        return run_init_command(repo_path);
    }
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);
    let test_section = load_test_section_config(cli.config.as_ref(), cli.defaults);
    dispatch(cli, &py_config, &rs_config, &gate_config, &test_section)
}

pub(crate) fn parse_cli() -> Cli {
    #[cfg(test)]
    {
        parse_cli_from(["kiss", "rules"])
    }
    #[cfg(not(test))]
    {
        parse_cli_from(std::env::args_os())
    }
}

pub(crate) fn parse_cli_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::parse_from(args)
}

#[cfg(test)]
mod run_coverage {
    use super::{parse_cli_from, run_cli_entrypoint, run_with_cli};
    use crate::bin_cli::args::{Cli, Commands};

    #[test]
    fn run_entrypoint_and_explicit_cli_paths_return_success() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let entry_tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            entry_tmp.path().join("sample.py"),
            "def f():\n    return 1\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(entry_tmp.path()).unwrap();
        assert_eq!(run_cli_entrypoint(), 0);
        assert!(entry_tmp.path().join(".kissconfig").exists());

        assert!(matches!(
            parse_cli_from(["kiss", "rules"]).command,
            Commands::Rules
        ));
        assert_eq!(
            run_with_cli(Cli {
                config: None,
                lang: None,
                defaults: true,
                command: Commands::Rules,
            }),
            0
        );
        std::env::set_current_dir(orig_dir).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join(".kissconfig");
        assert!(!config_path.exists());
        assert_eq!(
            run_with_cli(Cli {
                config: None,
                lang: None,
                defaults: true,
                command: Commands::Init {
                    repo_path: tmp.path().to_path_buf(),
                },
            }),
            0
        );
        assert!(
            config_path.exists(),
            "init should write a default config into the requested repo"
        );
    }
}
