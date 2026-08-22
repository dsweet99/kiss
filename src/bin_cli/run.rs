use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::config_session::{
    ensure_default_config_exists, ensure_default_config_from, load_configs, load_gate_config,
    load_test_section_config,
};
use crate::bin_cli::dispatch::dispatch;
use clap::Parser;

pub fn run_cli_entrypoint() -> i32 {
    run_with_cli(parse_cli())
}

#[cfg(test)]
pub use run_cli_entrypoint as run;

pub(crate) fn run_with_cli(cli: Cli) -> i32 {
    if let Commands::RustLlvmCovTargetRunner {
        output_dir,
        runner_map,
        platform,
        command,
    } = &cli.command
    {
        return rust_llvm_cov_runner::run_target_runner_shim(
            output_dir, runner_map, platform, command,
        );
    }
    if let Some(code) = prepare_watch_flags(&cli) {
        return code;
    }
    prepare_default_config(&cli);
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);
    let test_section = match load_test_section_config(cli.config.as_ref(), cli.defaults) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: {err}");
            return 2;
        }
    };
    dispatch(cli, &py_config, &rs_config, &gate_config, &test_section)
}

fn prepare_default_config(cli: &Cli) {
    match &cli.command {
        Commands::Check { paths, ignore, .. } => {
            ensure_default_config_from(paths, ignore);
        }
        _ => ensure_default_config_exists(),
    }
}

fn prepare_watch_flags(cli: &Cli) -> Option<i32> {
    let Commands::Test { watch, dry_run, .. } = &cli.command else {
        return None;
    };
    if *watch && *dry_run {
        eprintln!("error: kiss test: --watch cannot be combined with --dry-run");
        return Some(2);
    }
    None
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
    use std::fs;

    #[test]
    fn run_with_cli_rejects_watch_combined_with_dry_run() {
        let cli = parse_cli_from(["kiss", "test", "--watch", "--dry-run"]);
        assert_eq!(run_with_cli(cli), 2);
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
                defaults: true,
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
                defaults: false,
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
    fn hidden_rust_llvm_cov_target_runner_dispatches_before_config_loading() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("shim-child.sh");
        fs::write(&script, "#!/bin/sh\nexit 6\n").unwrap();
        make_executable(&script);
        let output_dir = tmp.path().join("instances");

        let runner_map = tmp.path().join("runner-map.json");
        fs::write(&runner_map, b"{}").unwrap();
        let code = run_with_cli(Cli {
            config: None,
            lang: None,
            defaults: true,
            command: Commands::RustLlvmCovTargetRunner {
                output_dir: output_dir.clone(),
                runner_map,
                platform: "x86_64-unknown-linux-gnu".to_string(),
                command: vec![script.into_os_string()],
            },
        });

        assert_eq!(code, 6);
        assert!(fs::read_dir(output_dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                == Some("json")
        }));
    }

    #[test]
    fn run_with_cli_rejects_invalid_test_num_jobs_config() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\nnum_jobs = 0\ntest_coverage_threshold = 0\n",
        )
        .unwrap();
        fs::write(tmp.path().join("sample.py"), "def f():\n    return 1\n").unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let code = run_with_cli(Cli {
            config: None,
            lang: None,
            defaults: false,
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
            "[global]\nduplication_enabled = false\n[test]\ntest_coverage_threshold = 0\n[python]\n[rust]\n",
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
                    defaults: false,
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
                defaults: false,
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

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &std::path::Path) {}
}
