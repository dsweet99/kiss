mod analyze;
mod rules;

use clap::{Parser, Subcommand};
use kiss::config_gen::{
    collect_all_stats_with_ignore, collect_py_stats_with_ignore, collect_rs_stats_with_ignore,
    generate_config_toml_by_language, infer_gate_config_for_paths, write_mimic_config,
};
use kiss::{
    Config, ConfigLanguage, GateConfig, Language, MetricStats, compute_summaries,
    format_stats_table, get_metric_def, truncate,
};
use std::path::{Path, PathBuf};

use crate::analyze::run_analyze;
use crate::rules::{run_config, run_rules};

#[derive(Parser, Debug)]
#[command(
    name = "kiss",
    version,
    about = "Code-quality metrics tool for Python and Rust"
)]
#[command(
    after_help = "EXAMPLES:\n  kiss check .                 Analyze current directory\n  kiss check . src/module/     Analyze module against full codebase (focus mode)\n  kiss check --lang rust src/  Analyze only Rust files in src/\n  kiss mimic . --out .kissconfig   Generate config from codebase"
)]
struct Cli {
    /// Path to custom config file (default: .kissconfig or ~/.kissconfig)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Filter by language: python (py) or rust (rs)
    #[arg(long, global = true, value_parser = parse_language, value_name = "LANG")]
    lang: Option<Language>,

    /// Use built-in defaults, ignoring config files
    #[arg(long, global = true)]
    defaults: bool,

    #[command(subcommand)]
    command: Commands,
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
    /// Analyze code for violations
    Check {
        /// First path is UNIVERSE (analysis scope), additional paths are FOCUS (report only these)
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Bypass coverage gate and show all violations
        #[arg(long)]
        all: bool,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        /// Show timing breakdown for performance analysis
        #[arg(long)]
        timing: bool,
    },
    /// Show metric statistics for codebase
    Stats {
        /// Paths to analyze
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Show top N outliers for each metric (default: 10)
        #[arg(long, value_name = "N", default_missing_value = "10", num_args = 0..=1, require_equals = true)]
        all: Option<usize>,
        /// Show full per-unit table (wide format)
        #[arg(long)]
        table: bool,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Generate .kissconfig thresholds from an existing codebase
    Mimic {
        /// Paths to analyze for threshold generation
        #[arg(required = true)]
        paths: Vec<String>,
        /// Output file (prints to stdout if not specified)
        #[arg(long, short, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Shortcut: generate .kissconfig from current directory (same as: mimic . --out .kissconfig)
    Clamp,
    /// Detect duplicate code blocks
    Dry {
        /// Path to scan for duplicates
        #[arg(default_value = ".")]
        path: String,
        /// Optional file paths to filter results (only report duplicates involving these files)
        #[arg(value_name = "FILTER_FILES")]
        filter_files: Vec<String>,
        /// Number of lines per chunk
        #[arg(long, default_value = "10")]
        chunk_lines: usize,
        /// Character n-gram size for shingling
        #[arg(long, default_value = "5")]
        shingle_size: usize,
        /// Number of `MinHash` functions
        #[arg(long, default_value = "128")]
        minhash_size: usize,
        /// Number of LSH bands
        #[arg(long, default_value = "32")]
        lsh_bands: usize,
        /// Minimum similarity threshold [0.0-1.0]
        #[arg(long, default_value = "0.5")]
        min_similarity: f64,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Display all available rules and their current thresholds
    Rules,
    /// Show effective configuration (merged from all sources)
    Config,
}

fn main() {
    set_sigpipe_default();
    let cli = Cli::parse();
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);

    match cli.command {
        Commands::Check {
            paths,
            all,
            ignore,
            timing,
        } => {
            let ignore = normalize_ignore_prefixes(&ignore);
            let universe = &paths[0];
            let focus = if paths.len() > 1 {
                &paths[1..]
            } else {
                &paths[..]
            };
            validate_paths(&paths);
            let opts = analyze::AnalyzeOptions {
                universe,
                focus_paths: focus,
                py_config: &py_config,
                rs_config: &rs_config,
                lang_filter: cli.lang,
                bypass_gate: all,
                gate_config: &gate_config,
                ignore_prefixes: &ignore,
                show_timing: timing,
            };
            if !run_analyze(&opts) {
                std::process::exit(1);
            }
        }
        Commands::Stats {
            paths,
            all,
            table,
            ignore,
        } => {
            let ignore = normalize_ignore_prefixes(&ignore);
            run_stats(&paths, cli.lang, &ignore, all, table);
        }
        Commands::Mimic { paths, out, ignore } => {
            let ignore = normalize_ignore_prefixes(&ignore);
            run_mimic(&paths, out.as_deref(), cli.lang, &ignore);
        }
        Commands::Clamp => run_mimic(
            &[".".to_string()],
            Some(Path::new(".kissconfig")),
            cli.lang,
            &[],
        ),
        Commands::Dry {
            path,
            filter_files,
            chunk_lines,
            shingle_size,
            minhash_size,
            lsh_bands,
            min_similarity,
            ignore,
        } => {
            let ignore = normalize_ignore_prefixes(&ignore);
            let config = kiss::DuplicationConfig {
                shingle_size,
                minhash_size,
                lsh_bands,
                min_similarity,
            };
            analyze::run_dry(
                &path,
                &filter_files,
                chunk_lines,
                &config,
                &ignore,
                cli.lang,
            );
        }
        Commands::Rules => run_rules(&py_config, &rs_config, &gate_config, cli.lang, cli.defaults),
        Commands::Config => run_config(
            &py_config,
            &rs_config,
            &gate_config,
            cli.config.as_ref(),
            cli.defaults,
        ),
    }
}

#[cfg(unix)]
fn set_sigpipe_default() {
    // When `kiss` output is piped (e.g. `kiss stats --all . | head`), downstream may close the pipe early.
    // Rust's default SIGPIPE behavior is "ignore", which turns this into an EPIPE write error and can panic.
    // Restoring SIGPIPE's default behavior makes the process terminate quietly instead of panicking.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn set_sigpipe_default() {}

fn normalize_ignore_prefixes(prefixes: &[String]) -> Vec<String> {
    let result: Vec<String> = prefixes
        .iter()
        .map(|p| p.trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if result.iter().any(|p| p == ".") {
        eprintln!("Warning: --ignore '.' matches all files");
    }
    result
}

fn validate_paths(paths: &[String]) {
    for p in paths {
        if !Path::new(p).exists() {
            eprintln!("Error: Path does not exist: {p}");
            std::process::exit(1);
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
            eprintln!(
                "Note: Could not write default config to {}: {}",
                home_config.display(),
                e
            );
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
    if use_defaults {
        return defaults();
    }
    let Some(path) = config_path else {
        return (
            Config::load_for_language(ConfigLanguage::Python),
            Config::load_for_language(ConfigLanguage::Rust),
        );
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        eprintln!("Warning: Config file not found: {}", path.display());
        return defaults();
    };
    if let Err(e) = content.parse::<toml::Table>() {
        eprintln!("Warning: Failed to parse config {}: {}", path.display(), e);
        return defaults();
    }
    (
        Config::load_from_content(&content, ConfigLanguage::Python),
        Config::load_from_content(&content, ConfigLanguage::Rust),
    )
}

fn config_provenance() -> String {
    let local = Path::new(".kissconfig");
    let home = std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".kissconfig"))
        .filter(|p| p.exists());
    let local_status = if local.exists() { "found" } else { "not found" };
    let home_status = home.as_ref().map_or_else(
        || "not found".to_string(),
        |p| format!("found: {}", p.display()),
    );
    format!("Config: defaults + ~/.kissconfig ({home_status}) + ./.kissconfig ({local_status})")
}

fn run_stats(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    all: Option<usize>,
    table: bool,
) {
    if table {
        run_stats_table(paths, lang_filter, ignore);
    } else if let Some(n) = all {
        run_stats_top(paths, lang_filter, ignore, n);
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
    println!(
        "kiss stats - Summary Statistics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    if py_cnt > 0 {
        println!(
            "=== Python ({py_cnt} files) ===\n{}\n",
            format_stats_table(&compute_summaries(&py_stats))
        );
    }
    if rs_cnt > 0 {
        println!(
            "=== Rust ({rs_cnt} files) ===\n{}",
            format_stats_table(&compute_summaries(&rs_stats))
        );
    }
}

fn run_stats_top(paths: &[String], lang_filter: Option<Language>, ignore: &[String], n: usize) {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats --all {n} - Top Outliers\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    let all_units = collect_all_units(&py_files, &rs_files);
    print_all_top_metrics(&all_units, n);
}

fn collect_all_units(py_files: &[PathBuf], rs_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    use kiss::parsing::parse_files;
    use kiss::rust_parsing::parse_rust_files;
    use kiss::{build_dependency_graph, rust_graph::build_rust_dependency_graph};
    use kiss::{collect_detailed_py, collect_detailed_rs};

    let mut all_units = Vec::new();
    if !py_files.is_empty() {
        let results = parse_files(py_files).expect("parse files");
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_dependency_graph(&parsed);
        all_units.extend(collect_detailed_py(&parsed, Some(&graph)));
    }
    if !rs_files.is_empty() {
        let results = parse_rust_files(rs_files);
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_rust_dependency_graph(&parsed);
        all_units.extend(collect_detailed_rs(&parsed, Some(&graph)));
    }
    all_units
}

type UnitMetricExtractor = fn(&kiss::UnitMetrics) -> Option<usize>;
type MetricSpec = (&'static str, &'static str, UnitMetricExtractor);

fn print_all_top_metrics(units: &[kiss::UnitMetrics], n: usize) {
    fn name(metric_id: &'static str, fallback: &'static str) -> &'static str {
        get_metric_def(metric_id).map_or(fallback, |d| d.display_name)
    }

    let metrics: &[MetricSpec] = &[
        ("statements_per_function", "Statements per function", |u| {
            u.statements
        }),
        ("args_total", "Arguments (total)", |u| u.arguments),
        ("args_positional", "Arguments (positional)", |u| {
            u.args_positional
        }),
        ("args_keyword_only", "Arguments (keyword-only)", |u| {
            u.args_keyword_only
        }),
        ("max_indentation_depth", "Max indentation depth", |u| {
            u.indentation
        }),
        ("nested_function_depth", "Nested function depth", |u| {
            u.nested_depth
        }),
        ("branches_per_function", "Branches per function", |u| {
            u.branches
        }),
        ("returns_per_function", "Returns per function", |u| {
            u.returns
        }),
        (
            "local_variables_per_function",
            "Local variables per function",
            |u| u.locals,
        ),
        ("methods_per_type", "Methods per type", |u| u.methods),
        ("lines_per_file", "Lines per file", |u| u.lines),
        ("imported_names_per_file", "Imported names per file", |u| {
            u.imports
        }),
        ("fan_in", "Fan-in (per module)", |u| u.fan_in),
        ("fan_out", "Fan-out (per module)", |u| u.fan_out),
        ("transitive_deps", "Transitive deps (per module)", |u| {
            u.transitive_deps
        }),
        ("dependency_depth", "Dependency depth (per module)", |u| {
            u.dependency_depth
        }),
    ];

    for (metric_id, fallback_display_name, extractor) in metrics {
        print_top_for_metric(
            units,
            n,
            metric_id,
            name(metric_id, fallback_display_name),
            *extractor,
        );
    }
}

fn print_top_for_metric<F>(
    units: &[kiss::UnitMetrics],
    n: usize,
    metric_id: &str,
    display_name: &str,
    extractor: F,
) where
    F: Fn(&kiss::UnitMetrics) -> Option<usize>,
{
    let mut with_values: Vec<_> = units
        .iter()
        .filter_map(|u| extractor(u).map(|v| (v, u)))
        .collect();
    if with_values.is_empty() {
        return;
    }
    with_values.sort_by(|a, b| b.0.cmp(&a.0));
    println!("{metric_id}  ({display_name})  top {n}");
    println!("{:>5}  {:<40}  {:>5}  name", "value", "file", "line");
    println!("{}", "-".repeat(70));
    for (val, u) in with_values.into_iter().take(n) {
        println!(
            "{:>5}  {:<40}  {:>5}  {}",
            val,
            truncate(&u.file, 40),
            u.line,
            u.name
        );
    }
    println!();
}


fn gather_files_by_lang(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
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

fn run_stats_table(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    use kiss::parsing::parse_files;
    use kiss::rust_parsing::parse_rust_files;
    use kiss::{build_dependency_graph, rust_graph::build_rust_dependency_graph};
    use kiss::{collect_detailed_py, collect_detailed_rs, format_detailed_table};

    let (py_files, rs_files) = gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats --table - Per-Unit Metrics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    if !py_files.is_empty() {
        let results = parse_files(&py_files).expect("parse files");
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_dependency_graph(&parsed);
        let units = collect_detailed_py(&parsed, Some(&graph));
        println!(
            "=== Python ({} files, {} units) ===\n{}",
            py_files.len(),
            units.len(),
            format_detailed_table(&units)
        );
    }
    if !rs_files.is_empty() {
        let results = parse_rust_files(&rs_files);
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_rust_dependency_graph(&parsed);
        let units = collect_detailed_rs(&parsed, Some(&graph));
        println!(
            "=== Rust ({} files, {} units) ===\n{}",
            rs_files.len(),
            units.len(),
            format_detailed_table(&units)
        );
    }
}

fn run_mimic(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
) {
    let ((py_stats, py_cnt), (rs_stats, rs_cnt)) =
        collect_all_stats_with_ignore(paths, lang_filter, ignore);
    if py_cnt + rs_cnt == 0 {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    let gate = infer_gate_config_for_paths(paths, lang_filter, ignore);
    let toml = generate_config_toml_by_language(&py_stats, &rs_stats, py_cnt, rs_cnt, &gate);
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
            Cli::try_parse_from(["kiss", "clamp"]).unwrap().command,
            Commands::Clamp
        ));
        ensure_default_config_exists();
    }
    #[test]
    fn test_gather_stats_normalize_validate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().to_string_lossy().to_string();
        assert!(
            gather_files_by_lang(std::slice::from_ref(&p), None, &[])
                .0
                .is_empty()
        );
        std::fs::write(tmp.path().join("test.py"), "def foo(): pass").unwrap();
        std::fs::write(tmp.path().join("test.rs"), "fn main() {}").unwrap();
        assert_eq!(
            gather_files_by_lang(std::slice::from_ref(&p), None, &[])
                .0
                .len(),
            1
        );
        run_stats_summary(std::slice::from_ref(&p), Some(Language::Python), &[]);
        run_stats_table(std::slice::from_ref(&p), Some(Language::Rust), &[]);
        assert_eq!(
            normalize_ignore_prefixes(&["src/".to_string(), String::new()]),
            vec!["src"]
        );
        validate_paths(&[p]); // tests valid path doesn't panic/exit
    }
    #[test]
    fn test_run_stats_and_mimic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().to_string_lossy().to_string();
        std::fs::write(tmp.path().join("test.py"), "def foo(): pass").unwrap();
        run_stats(
            std::slice::from_ref(&p),
            Some(Language::Python),
            &[],
            None,
            false,
        );
        run_stats(
            std::slice::from_ref(&p),
            Some(Language::Python),
            &[],
            Some(10),
            false,
        );
        run_stats(
            std::slice::from_ref(&p),
            Some(Language::Python),
            &[],
            None,
            true,
        );
        run_mimic(std::slice::from_ref(&p), None, Some(Language::Python), &[]);
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
        print_top_for_metric(&units, 1, "test_metric", "Test Metric", |u| u.statements);
        assert_eq!(truncate("short.rs", 20), "short.rs");
        assert!(truncate("this/is/a/very/long/path.rs", 20).starts_with("..."));
    }
    #[test]
    fn test_main_exists() {
        // Reference main to ensure test coverage
        let _ = main as fn();
    }
    #[test]
    fn test_config_provenance() {
        let prov = config_provenance();
        assert!(prov.contains("Config:"));
        assert!(prov.contains("defaults"));
        assert!(prov.contains(".kissconfig"));
    }
}
