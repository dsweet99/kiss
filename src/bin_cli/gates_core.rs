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
use crate::bin_cli::util::validate_paths;
use kiss::Language;
use kiss::normalize_ignore_prefixes;
use kiss::truncate;
use kiss::{Config, ConfigLanguage, GateConfig};

#[test]
fn test_language_and_config() {
    assert_eq!(parse_language("python"), Ok(Language::Python));
    assert_eq!(parse_language("rust"), Ok(Language::Rust));
    assert!(parse_language("invalid").is_err());
    let (py, rs) = load_configs(None);
    assert!(py.statements_per_function > 0 && rs.statements_per_function > 0);
    let tmp = tempfile::TempDir::new().unwrap();
    let builtin = tmp.path().join("builtin.toml");
    std::fs::write(&builtin, "[python]\n[rust]\n").unwrap();
    let (py_def, _) = load_configs(Some(&builtin));
    assert_eq!(
        py_def.statements_per_function,
        kiss::defaults::python::STATEMENTS_PER_FUNCTION
    );
    let path = tmp.path().join("kiss.toml");
    std::fs::write(&path, "[test]\ntest_coverage_threshold = 80\n").unwrap();
    assert_eq!(load_gate_config(Some(&path)).test_coverage_threshold, 80);
    assert_eq!(
        load_gate_config(Some(&builtin)).test_coverage_threshold,
        kiss::defaults::gate::TEST_COVERAGE_THRESHOLD
    );
}

#[test]
fn test_clamp_keeps_default_coverage_threshold() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("app.py"),
        "def covered():\n    return 1\n\ndef uncovered():\n    return 2\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_app.py"),
        "from app import covered\n\ndef test_covered():\n    assert covered() == 1\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("fake_extra.py"),
        "def ignored_uncovered():\n    return 3\n",
    )
    .unwrap();

    let ignore = crate::bin_cli::util::merge_check_ignore_prefixes(&[]);
    let path = tmp.path().to_string_lossy().to_string();
    let gate = kiss::config_gen::infer_gate_config_for_paths(&[path], None, &ignore).unwrap();
    assert_eq!(
        gate.test_coverage_threshold,
        kiss::defaults::gate::TEST_COVERAGE_THRESHOLD,
        "clamp must not infer coverage from static references after the kiss cov split"
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
        Cli::try_parse_from(["kiss", "check"]).unwrap().command,
        Commands::Check { .. }
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
    run_stats_summary(&RunStatsArgs {
        paths: std::slice::from_ref(&p),
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: None,
        table: false,
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        gate_config: &gate_cfg,
        language_tables: kiss::LanguageTablesPresent::both(),
        config: None,
    });
    run_stats_table(
        std::slice::from_ref(&p),
        Some(Language::Rust),
        &[],
        kiss::LanguageTablesPresent::both(),
    );
    assert_eq!(
        normalize_ignore_prefixes(&["src/".to_string(), String::new()]),
        vec!["src"]
    );
    validate_paths(&[p]);
}

fn exercise_stats_modes_and_mimic(p: &str) {
    let gate = kiss::GateConfig::default();
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
        gate_config: &gate,
        language_tables: kiss::LanguageTablesPresent::both(),
        config: None,
    });
    run_stats(RunStatsArgs {
        paths,
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: Some(10),
        table: false,
        py_config: &kiss::Config::python_defaults(),
        rs_config: &kiss::Config::rust_defaults(),
        gate_config: &gate,
        language_tables: kiss::LanguageTablesPresent::both(),
        config: None,
    });
    run_stats(RunStatsArgs {
        paths,
        lang_filter: Some(Language::Python),
        ignore: &[],
        all: None,
        table: true,
        py_config: &kiss::Config::python_defaults(),
        rs_config: &kiss::Config::rust_defaults(),
        gate_config: &gate,
        language_tables: kiss::LanguageTablesPresent::both(),
        config: None,
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
