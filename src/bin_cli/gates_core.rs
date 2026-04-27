use kiss::{Config, ConfigLanguage, GateConfig};
use kiss::Language;
use crate::bin_cli::args::{Cli, Commands, parse_language};
use crate::bin_cli::config_session::{
    ensure_default_config_exists, load_configs, load_gate_config,
};
use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::run::run;
use crate::bin_cli::stats::{
    RunStatsArgs, collect_all_units, print_all_top_metrics, print_top_for_metric, run_stats,
    run_stats_summary, run_stats_table,
};
use crate::bin_cli::util::{normalize_ignore_prefixes, validate_paths};
use kiss::truncate;

#[test]
fn test_language_and_config() {
    assert_eq!(parse_language("python"), Ok(Language::Python));
    assert_eq!(parse_language("rust"), Ok(Language::Rust));
    assert!(parse_language("invalid").is_err());
    let (py, rs) = load_configs(None, false);
    assert!(py.statements_per_function > 0 && rs.statements_per_function > 0);
    let (py_def, _) = load_configs(None, true);
    assert_eq!(
        py_def.statements_per_function,
        kiss::defaults::python::STATEMENTS_PER_FUNCTION
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("kiss.toml");
    std::fs::write(&path, "[gate]\ntest_coverage_threshold = 80\n").unwrap();
    assert_eq!(
        load_gate_config(Some(&path), false).test_coverage_threshold,
        80
    );
    assert_eq!(
        load_gate_config(Some(&path), true).test_coverage_threshold,
        kiss::defaults::gate::TEST_COVERAGE_THRESHOLD
    );
}

#[test]
fn test_cli_and_commands() {
    use clap::Parser;
    assert!(matches!(
        Cli::try_parse_from(["kiss", "check", "."]).unwrap().command,
        Commands::Check { .. }
    ));
    assert!(matches!(
        Cli::try_parse_from(["kiss", "rules"]).unwrap().command,
        Commands::Rules
    ));
    assert!(matches!(
        Cli::try_parse_from(["kiss", "stats"]).unwrap().command,
        Commands::Stats { .. }
    ));
    assert!(matches!(
        Cli::try_parse_from(["kiss", "mv", "src/a.py::foo", "bar"])
            .unwrap()
            .command,
        Commands::Mv { .. }
    ));
    assert!(matches!(
        Cli::try_parse_from(["kiss", "clamp"]).unwrap().command,
        Commands::Clamp { .. }
    ));
    ensure_default_config_exists();
}

#[test]
fn test_gather_stats_normalize_validate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let p = tmp.path().to_string_lossy().to_string();
    assert!(
        kiss::discovery::gather_files_by_lang(std::slice::from_ref(&p), None, &[])
            .0
            .is_empty()
    );
    std::fs::write(tmp.path().join("test.py"), "def foo(): pass").unwrap();
    std::fs::write(tmp.path().join("test.rs"), "fn main() {}").unwrap();
    assert_eq!(
        kiss::discovery::gather_files_by_lang(std::slice::from_ref(&p), None, &[])
            .0
            .len(),
        1
    );
    let py_cfg = Config::load_for_language(ConfigLanguage::Python);
    let rs_cfg = Config::load_for_language(ConfigLanguage::Rust);
    let gate_cfg = GateConfig::load();
    run_stats_summary(
        std::slice::from_ref(&p),
        Some(Language::Python),
        &[],
        &py_cfg,
        &rs_cfg,
        &gate_cfg,
    );
    run_stats_table(std::slice::from_ref(&p), Some(Language::Rust), &[]);
    assert_eq!(
        normalize_ignore_prefixes(&["src/".to_string(), String::new()]),
        vec!["src"]
    );
    validate_paths(&[p]);
}

fn exercise_stats_modes_and_mimic(p: &str) {
    let p_owned = p.to_string();
    let paths = std::slice::from_ref(&p_owned);
    run_stats(RunStatsArgs {
        paths,
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: None,
        table: false,
        py_config: &kiss::Config::python_defaults(),
        rs_config: &kiss::Config::rust_defaults(),
        gate_config: &kiss::GateConfig::default(),
    });
    run_stats(RunStatsArgs {
        paths,
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: Some(10),
        table: false,
        py_config: &kiss::Config::python_defaults(),
        rs_config: &kiss::Config::rust_defaults(),
        gate_config: &kiss::GateConfig::default(),
    });
    run_stats(RunStatsArgs {
        paths,
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: None,
        table: true,
        py_config: &kiss::Config::python_defaults(),
        rs_config: &kiss::Config::rust_defaults(),
        gate_config: &kiss::GateConfig::default(),
    });
    run_mimic(paths, None, Some(Language::Python), &[]);
}

#[test]
fn test_run_stats_and_mimic() {
    let tmp = tempfile::TempDir::new().unwrap();
    let p = tmp.path().to_string_lossy().to_string();
    std::fs::write(tmp.path().join("test.py"), "def foo(): pass").unwrap();
    exercise_stats_modes_and_mimic(&p);
}

#[test]
fn test_stats_top_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    x = 1\n    y = 2").unwrap();
    std::fs::write(tmp.path().join("b.rs"), "fn bar() { let z = 3; }").unwrap();
    let py_files = vec![tmp.path().join("a.py")];
    let rs_files = vec![tmp.path().join("b.rs")];
    let units = collect_all_units(&py_files, &rs_files);
    assert!(!units.is_empty());
    print_all_top_metrics(&units, 2);
    print_top_for_metric(&units, 1, "test_metric", |u| u.statements);
    assert_eq!(truncate("short.rs", 20), "short.rs");
    assert!(truncate("this/is/a/very/long/path.rs", 20).starts_with("..."));
}

#[test]
fn test_run_entrypoint_exists() {
    let _ = run as fn() -> i32;
}
