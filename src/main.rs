#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::format_push_string)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::needless_update)]
#![allow(clippy::iter_on_single_items)]
#![allow(clippy::float_cmp)]
#![allow(clippy::ref_option)]

mod analyze;
mod rules;

use clap::{Parser, Subcommand};
use kiss::{
    compute_summaries, format_stats_table, Config, ConfigLanguage, GateConfig, Language,
    MetricStats,
};
use kiss::config_gen::{
    collect_all_stats_with_ignore, collect_py_stats_with_ignore, collect_rs_stats_with_ignore,
    generate_config_toml_by_language, write_mimic_config,
};
use std::path::{Path, PathBuf};

use crate::analyze::run_analyze;
use crate::rules::run_rules;

#[derive(Parser, Debug)]
#[command(name = "kiss", version, about = "Code-quality metrics tool for Python and Rust")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true, value_parser = parse_language)]
    lang: Option<Language>,

    #[arg(long, global = true)]
    all: bool,

    #[arg(long, global = true)]
    defaults: bool,

    #[arg(long, global = true)]
    ignore: Vec<String>,

    #[arg(long, global = true)]
    warnings: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(default_value = ".")]
    path: String,
}

fn parse_language(s: &str) -> Result<Language, String> {
    match s.to_lowercase().as_str() {
        "python" | "py" => Ok(Language::Python),
        "rust" | "rs" => Ok(Language::Rust),
        _ => Err(format!("Unknown language '{s}'. Use 'python' or 'rust'.")),
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    Stats {
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },
    Mimic {
        #[arg(required = true)]
        paths: Vec<String>,
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    Rules,
}

fn main() {
    let cli = Cli::parse();
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(&cli.config, cli.defaults);
    let gate_config = load_gate_config(&cli.config, cli.defaults);

    match cli.command {
        Some(Commands::Stats { paths }) => run_stats(&paths, cli.lang, &cli.ignore, cli.all),
        Some(Commands::Mimic { paths, out }) => run_mimic(&paths, out.as_deref(), cli.lang, &cli.ignore),
        Some(Commands::Rules) => run_rules(&py_config, &rs_config, &gate_config, cli.lang, cli.defaults),
        None => {
            if !run_analyze(&cli.path, &py_config, &rs_config, cli.lang, cli.all, &gate_config, &cli.ignore, cli.warnings) {
                std::process::exit(1);
            }
        }
    }
}

fn ensure_default_config_exists() {
    let local_config = Path::new(".kissconfig");
    if local_config.exists() {
        return;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home_config = Path::new(&home).join(".kissconfig");
        if !home_config.exists()
            && let Err(e) = std::fs::write(&home_config, kiss::default_config_toml())
        {
            eprintln!("Note: Could not write default config to {}: {}", home_config.display(), e);
        }
    }
}

fn load_gate_config(config_path: &Option<PathBuf>, use_defaults: bool) -> GateConfig {
    if use_defaults {
        GateConfig::default()
    } else if let Some(path) = config_path {
        GateConfig::load_from(path)
    } else {
        GateConfig::load()
    }
}

fn load_configs(config_path: &Option<PathBuf>, use_defaults: bool) -> (Config, Config) {
    if use_defaults {
        (Config::python_defaults(), Config::rust_defaults())
    } else if let Some(path) = config_path {
        (
            Config::load_from_for_language(path, ConfigLanguage::Python),
            Config::load_from_for_language(path, ConfigLanguage::Rust),
        )
    } else {
        (
            Config::load_for_language(ConfigLanguage::Python),
            Config::load_for_language(ConfigLanguage::Rust),
        )
    }
}

fn run_stats(paths: &[String], lang_filter: Option<Language>, ignore: &[String], show_all: bool) {
    if show_all {
        run_stats_detailed(paths, lang_filter, ignore);
    } else {
        run_stats_summary(paths, lang_filter, ignore);
    }
}

fn run_stats_summary(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let (mut py_stats, mut rs_stats) = (MetricStats::default(), MetricStats::default());
    let (mut py_cnt, mut rs_cnt) = (0, 0);
    for path in paths {
        let root = Path::new(path);
        if lang_filter.is_none() || lang_filter == Some(Language::Python) {
            let (s, c) = collect_py_stats_with_ignore(root, ignore);
            py_stats.merge(s);
            py_cnt += c;
        }
        if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
            let (s, c) = collect_rs_stats_with_ignore(root, ignore);
            rs_stats.merge(s);
            rs_cnt += c;
        }
    }
    if py_cnt + rs_cnt == 0 {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!("kiss stats - Summary Statistics\nAnalyzed from: {}\n", paths.join(", "));
    if py_cnt > 0 {
        println!("=== Python ({py_cnt} files) ===\n{}\n", format_stats_table(&compute_summaries(&py_stats)));
    }
    if rs_cnt > 0 {
        println!("=== Rust ({rs_cnt} files) ===\n{}", format_stats_table(&compute_summaries(&rs_stats)));
    }
}

fn gather_files_by_lang(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    use kiss::discovery::find_source_files_with_ignore;
    let (mut py_files, mut rs_files) = (Vec::new(), Vec::new());
    for path in paths {
        for sf in find_source_files_with_ignore(Path::new(path), ignore) {
            match (sf.language, lang_filter) {
                (Language::Python, None | Some(Language::Python)) => py_files.push(sf.path),
                (Language::Rust, None | Some(Language::Rust)) => rs_files.push(sf.path),
                _ => {}
            }
        }
    }
    (py_files, rs_files)
}

fn run_stats_detailed(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    use kiss::{collect_detailed_py, collect_detailed_rs, format_detailed_table};
    use kiss::{build_dependency_graph, rust_graph::build_rust_dependency_graph};
    use kiss::parsing::parse_files;
    use kiss::rust_parsing::parse_rust_files;

    let (py_files, rs_files) = gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!("kiss stats --all - Detailed Metrics\nAnalyzed from: {}\n", paths.join(", "));
    if !py_files.is_empty() {
        let results = parse_files(&py_files).expect("parse files");
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_dependency_graph(&parsed);
        let units = collect_detailed_py(&parsed, Some(&graph));
        println!("=== Python ({} files, {} units) ===\n{}", py_files.len(), units.len(), format_detailed_table(&units));
    }
    if !rs_files.is_empty() {
        let results = parse_rust_files(&rs_files);
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_rust_dependency_graph(&parsed);
        let units = collect_detailed_rs(&parsed, Some(&graph));
        println!("=== Rust ({} files, {} units) ===\n{}", rs_files.len(), units.len(), format_detailed_table(&units));
    }
}

fn run_mimic(paths: &[String], out: Option<&Path>, lang_filter: Option<Language>, ignore: &[String]) {
    let ((py_stats, py_cnt), (rs_stats, rs_cnt)) = collect_all_stats_with_ignore(paths, lang_filter, ignore);
    if py_cnt + rs_cnt == 0 {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    let toml = generate_config_toml_by_language(&py_stats, &rs_stats, py_cnt, rs_cnt);
    match out {
        Some(p) => write_mimic_config(p, &toml, py_cnt, rs_cnt),
        None => print!("{toml}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_language_parsing() {
        assert_eq!(parse_language("python"), Ok(Language::Python));
        assert_eq!(parse_language("rust"), Ok(Language::Rust));
        assert!(parse_language("invalid").is_err());
    }

    #[test]
    fn test_load_configs() {
        let (py, rs) = load_configs(&None, false);
        assert!(py.statements_per_function > 0 && rs.statements_per_function > 0);
        let (py_def, rs_def) = load_configs(&None, true);
        assert_eq!(py_def.statements_per_function, kiss::defaults::python::STATEMENTS_PER_FUNCTION);
        assert_eq!(rs_def.statements_per_function, kiss::defaults::rust::STATEMENTS_PER_FUNCTION);
    }

    #[test]
    fn test_gate_config_loading() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("kiss.toml");
        std::fs::write(&path, "[gate]\ntest_coverage_threshold = 80\n").unwrap();
        assert_eq!(load_gate_config(&Some(path.clone()), false).test_coverage_threshold, 80);
        assert_eq!(load_gate_config(&Some(path), true).test_coverage_threshold, kiss::defaults::gate::TEST_COVERAGE_THRESHOLD);
    }

    #[test]
    fn test_fn_pointers() {
        let _ = run_stats as fn(&[String], Option<Language>, &[String], bool);
        let _ = run_mimic as fn(&[String], Option<&Path>, Option<Language>, &[String]);
        let _ = main as fn();
    }

    #[test]
    fn test_cli_struct() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["kiss", "."]).unwrap();
        assert_eq!(cli.path, ".");
    }

    #[test]
    fn test_commands_enum() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["kiss", "rules"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Rules)));
    }

    #[test]
    fn test_ensure_default_config_exists() {
        ensure_default_config_exists();
    }
}
