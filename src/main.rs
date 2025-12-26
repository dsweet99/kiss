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
use crate::rules::{run_config, run_rules};

#[derive(Parser, Debug)]
#[command(name = "kiss", version, about = "Code-quality metrics tool for Python and Rust")]
#[command(after_help = "EXAMPLES:\n  kiss .                    Analyze current directory\n  kiss . src/module/        Analyze module against full codebase (focus mode)\n  kiss --lang rust src/     Analyze only Rust files in src/\n  kiss mimic . --out .kissconfig   Generate config from codebase")]
struct Cli {
    /// Path to custom config file (default: .kissconfig or ~/.kissconfig)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Filter by language: python (py) or rust (rs)
    #[arg(long, global = true, value_parser = parse_language, value_name = "LANG")]
    lang: Option<Language>,

    /// Bypass coverage gate and show all results (for exploration)
    #[arg(long, global = true)]
    all: bool,

    /// Use built-in defaults, ignoring config files
    #[arg(long, global = true)]
    defaults: bool,

    /// Ignore files/directories starting with PREFIX (repeatable)
    #[arg(long, global = true, value_name = "PREFIX")]
    ignore: Vec<String>,

    /// Show test coverage warnings for unreferenced code
    #[arg(long, global = true)]
    warnings: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Paths to analyze: [UNIVERSE] [FOCUS...]. UNIVERSE defines scope for graph
    /// and test discovery. FOCUS paths (if provided) restrict where violations
    /// are reported and coverage is enforced. Use focus mode for gradual adoption.
    #[arg(default_value = ".")]
    paths: Vec<String>,
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
    /// Show metric statistics for codebase (summary by default, --all for details)
    Stats {
        /// Paths to analyze
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },
    /// Generate .kissconfig thresholds from an existing codebase
    Mimic {
        /// Paths to analyze for threshold generation
        #[arg(required = true)]
        paths: Vec<String>,
        /// Output file (prints to stdout if not specified)
        #[arg(long, short, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Display all available rules and their current thresholds
    Rules,
    /// Show effective configuration (merged from all sources)
    Config,
}

fn main() {
    let cli = Cli::parse();
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);
    let ignore = normalize_ignore_prefixes(&cli.ignore);

    match cli.command {
        Some(Commands::Stats { paths }) => run_stats(&paths, cli.lang, &ignore, cli.all),
        Some(Commands::Mimic { paths, out }) => run_mimic(&paths, out.as_deref(), cli.lang, &ignore),
        Some(Commands::Rules) => run_rules(&py_config, &rs_config, &gate_config, cli.lang, cli.defaults),
        Some(Commands::Config) => run_config(&py_config, &rs_config, &gate_config, cli.config.as_ref(), cli.defaults),
        None => {
            let universe = &cli.paths[0];
            let focus = if cli.paths.len() > 1 { &cli.paths[1..] } else { &cli.paths[..] };
            validate_paths(&cli.paths);
            let opts = analyze::AnalyzeOptions {
                universe, focus_paths: focus, py_config: &py_config, rs_config: &rs_config,
                lang_filter: cli.lang, bypass_gate: cli.all, gate_config: &gate_config,
                ignore_prefixes: &ignore, show_warnings: cli.warnings,
            };
            if !run_analyze(&opts) {
                std::process::exit(1);
            }
        }
    }
}

fn normalize_ignore_prefixes(prefixes: &[String]) -> Vec<String> {
    prefixes.iter().map(|p| p.trim_end_matches('/').to_string()).filter(|p| !p.is_empty()).collect()
}

fn validate_paths(paths: &[String]) {
    for p in paths { if !Path::new(p).exists() { eprintln!("Error: Path does not exist: {p}"); std::process::exit(1); } }
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

fn load_gate_config(config_path: Option<&PathBuf>, use_defaults: bool) -> GateConfig {
    if use_defaults {
        GateConfig::default()
    } else if let Some(path) = config_path {
        GateConfig::load_from(path)
    } else {
        GateConfig::load()
    }
}

fn load_configs(config_path: Option<&PathBuf>, use_defaults: bool) -> (Config, Config) {
    let defaults = || (Config::python_defaults(), Config::rust_defaults());
    if use_defaults { return defaults(); }
    let Some(path) = config_path else {
        return (Config::load_for_language(ConfigLanguage::Python), Config::load_for_language(ConfigLanguage::Rust));
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        eprintln!("Warning: Config file not found: {}", path.display());
        return defaults();
    };
    if let Err(e) = content.parse::<toml::Table>() {
        eprintln!("Warning: Failed to parse config {}: {}", path.display(), e);
        return defaults();
    }
    (Config::load_from_content(&content, ConfigLanguage::Python), Config::load_from_content(&content, ConfigLanguage::Rust))
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
    #[test]
    fn test_language_and_config() {
        assert_eq!(parse_language("python"), Ok(Language::Python));
        assert_eq!(parse_language("rust"), Ok(Language::Rust));
        assert!(parse_language("invalid").is_err());
        let (py, rs) = load_configs(None, false);
        assert!(py.statements_per_function > 0 && rs.statements_per_function > 0);
        let (py_def, _) = load_configs(None, true);
        assert_eq!(py_def.statements_per_function, kiss::defaults::python::STATEMENTS_PER_FUNCTION);
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("kiss.toml");
        std::fs::write(&path, "[gate]\ntest_coverage_threshold = 80\n").unwrap();
        assert_eq!(load_gate_config(Some(&path), false).test_coverage_threshold, 80);
        assert_eq!(load_gate_config(Some(&path), true).test_coverage_threshold, kiss::defaults::gate::TEST_COVERAGE_THRESHOLD);
    }
    #[test]
    fn test_cli_and_commands() {
        use clap::Parser;
        assert_eq!(Cli::try_parse_from(["kiss", "."]).unwrap().paths, vec!["."]);
        assert!(matches!(Cli::try_parse_from(["kiss", "rules"]).unwrap().command, Some(Commands::Rules)));
        assert_eq!(Cli::try_parse_from(["kiss", ".", "src/", "lib/"]).unwrap().paths, vec![".", "src/", "lib/"]);
        ensure_default_config_exists();
    }
    #[test]
    fn test_gather_and_run_stats() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().to_string_lossy().to_string();
        let (py, rs) = gather_files_by_lang(std::slice::from_ref(&p), None, &[]);
        assert!(py.is_empty() && rs.is_empty());
        std::fs::write(tmp.path().join("test.py"), "def foo(): pass").unwrap();
        std::fs::write(tmp.path().join("test.rs"), "fn main() {}").unwrap();
        let (py2, rs2) = gather_files_by_lang(std::slice::from_ref(&p), None, &[]);
        assert_eq!((py2.len(), rs2.len()), (1, 1));
        run_stats_summary(std::slice::from_ref(&p), Some(Language::Python), &[]);
        run_stats_detailed(std::slice::from_ref(&p), Some(Language::Rust), &[]);
    }
}
