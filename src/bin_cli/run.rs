use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::config_session::{
    ensure_check_config_from, ensure_default_config_exists, load_configs, load_gate_config,
    load_test_section_config,
};
use crate::bin_cli::dispatch::dispatch;
use clap::Parser;

pub fn run_cli_entrypoint() -> i32 {
    run_with_cli(parse_cli())
}

pub(crate) fn run_with_cli(cli: Cli) -> i32 {
    let _config_override = kiss::ConfigPathOverrideGuard::enter(cli.config.as_deref());
    prepare_default_config(&cli);
    let (py_config, rs_config) = match load_configs(cli.config.as_ref()) {
        Ok(configs) => configs,
        Err(err) => {
            eprintln!("Error: {err}");
            return 2;
        }
    };
    let gate_config = match load_gate_config(cli.config.as_ref()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: {err}");
            return 2;
        }
    };
    let test_section = match load_test_section_config(cli.config.as_ref()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: {err}");
            return 2;
        }
    };
    dispatch(cli, &py_config, &rs_config, &gate_config, &test_section)
}

fn prepare_default_config(cli: &Cli) {
    if cli.config.is_some() {
        return;
    }
    match &cli.command {
        Commands::Check { paths, ignore, .. } => {
            ensure_check_config_from(paths, ignore);
        }
        _ => ensure_default_config_exists(),
    }
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
mod run_touch {
    use super::{parse_cli_from, run_cli_entrypoint, run_with_cli};
    use crate::bin_cli::args::{Cli, Commands};
    use std::fs;

    #[test]
    fn run_with_cli_rejects_watch_combined_with_dry_run() {
        use clap::Parser;
        assert!(Cli::try_parse_from(["kiss", "test", "--watch", "--dry-run"]).is_err());
    }

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
                command: Commands::Rules,
            }),
            0
        );
        std::env::set_current_dir(&orig_dir).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("ok.py"), "def f():\n    return 1\n").unwrap();
        let config_path = tmp.path().join(".kissconfig");
        assert!(!config_path.exists());
        std::env::set_current_dir(tmp.path()).unwrap();
        assert_eq!(
            run_with_cli(Cli {
                config: None,
                lang: None,
                command: Commands::Check {
                    paths: vec![".".to_string()],
                    ignore: Vec::new(),
                    timing: false,
                },
            }),
            0
        );
        assert!(
            config_path.exists(),
            "check should write .kissconfig when it is missing"
        );
        std::env::set_current_dir(&orig_dir).unwrap();
    }

    #[test]
    fn check_raises_rust_include_rollup_so_the_check_passes() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "struct Parent;\ninclude!(\"frag.rs\");\n",
        )
        .unwrap();
        fs::write(tmp.path().join("src/frag.rs"), "struct Frag;\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let code = run_with_cli(Cli {
            config: None,
            lang: None,
            command: Commands::Check {
                paths: vec![".".to_string()],
                ignore: Vec::new(),
                timing: false,
            },
        });
        let created = fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(&orig_dir).unwrap();
        assert_eq!(
            code, 0,
            "include rollup must pass after the raise:\n{created}"
        );
        assert!(
            created.contains("concrete_types_per_file = 2"),
            "rollup of two structs must raise the threshold:\n{created}"
        );
        assert!(
            created.contains("duplication_enabled = true"),
            "non-threshold settings must stay at kissconfig-default:\n{created}"
        );
    }

    #[test]
    fn run_with_cli_rejects_invalid_test_num_jobs_config() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".kissconfig"), "[test]\nnum_jobs = 0\n").unwrap();
        fs::write(tmp.path().join("sample.py"), "def f():\n    return 1\n").unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let code = run_with_cli(Cli {
            config: None,
            lang: None,
            command: Commands::Rules,
        });

        std::env::set_current_dir(original).unwrap();
        assert_eq!(code, 2);
    }

    #[test]
    fn run_with_cli_rejects_unknown_config_section() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(".kissconfig"),
            "bogus = 1\n\n[python]\nstatements_per_function = 1\n",
        )
        .unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let code = run_with_cli(Cli {
            config: None,
            lang: None,
            command: Commands::Rules,
        });

        std::env::set_current_dir(original).unwrap();
        assert_eq!(code, 2);
    }

    #[test]
    fn run_with_cli_exercises_primary_commands_on_mixed_fixture() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\n[python]\n[rust]\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("app.py"),
            "import helper\n\n\ndef f(x):\n    return helper.g(x) + 1\n",
        )
        .unwrap();
        fs::write(tmp.path().join("helper.py"), "def g(x):\n    return x\n").unwrap();
        fs::write(
            tmp.path().join("lib.rs"),
            "mod helper;\npub fn f(x: i32) -> i32 { helper::g(x) + 1 }\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("helper.rs"),
            "pub fn g(x: i32) -> i32 { x }\n",
        )
        .unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        for command in [
            Commands::Check {
                paths: vec![".".to_string()],
                ignore: Vec::new(),
                timing: true,
            },
            Commands::Stats {
                paths: vec![".".to_string()],
                all: Some(3),
                table: true,
                ignore: Vec::new(),
            },
            Commands::Dry {
                path: ".".to_string(),
                filter_files: Vec::new(),
                shingle_size: 3,
                minhash_size: 8,
                lsh_bands: 2,
                min_similarity: Some(0.9),
                ignore: Vec::new(),
            },
            Commands::Rules,
        ] {
            assert_eq!(
                run_with_cli(Cli {
                    config: None,
                    lang: None,
                    command,
                }),
                0
            );
        }

        let viz_out = tmp.path().join("graph.mmd");
        assert_eq!(
            run_with_cli(Cli {
                config: None,
                lang: None,
                command: Commands::Viz {
                    out: viz_out.clone(),
                    paths: vec![".".to_string()],
                    zoom: 1.0,
                    num_nodes: None,
                    ignore: Vec::new(),
                },
            }),
            0
        );
        assert!(fs::read_to_string(&viz_out).unwrap().contains("graph"));

        std::env::set_current_dir(original).unwrap();
    }

    #[test]
    fn run_with_cli_config_does_not_write_local_kissconfig() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("ok.py"), "def f():\n    return 1\n").unwrap();
        let custom = tmp.path().join("custom.toml");
        fs::write(&custom, "[python]\n[rust]\n").unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        assert_eq!(
            run_with_cli(Cli {
                config: Some(custom),
                lang: None,
                command: Commands::Rules,
            }),
            0
        );
        std::env::set_current_dir(original).unwrap();
        assert!(
            !tmp.path().join(".kissconfig").exists(),
            "--config should not write .kissconfig"
        );
    }
}
