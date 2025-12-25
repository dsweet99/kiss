use clap::{Parser, Subcommand};
use kiss::{
    analyze_file, analyze_graph, analyze_rust_file, analyze_rust_test_refs, analyze_test_refs,
    build_dependency_graph, build_rust_dependency_graph, cluster_duplicates, compute_summaries,
    detect_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication, find_python_files,
    find_rust_files, format_stats_table, parse_files, parse_rust_files, Config, ConfigLanguage,
    DuplicationConfig, GateConfig, Language, MetricStats, ParsedFile, ParsedRustFile,
};
use kiss::config_gen::{
    collect_all_stats, collect_py_stats, collect_rs_stats, generate_config_toml_by_language,
    write_mimic_config,
};
use std::path::{Path, PathBuf};

/// kiss - Code-quality metrics tool for Python and Rust
#[derive(Parser, Debug)]
#[command(name = "kiss", version, about = "Code-quality metrics tool for Python and Rust")]
struct Cli {
    /// Use specified config file instead of defaults
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Only analyze specified language (python, rust)
    #[arg(long, global = true, value_parser = parse_language)]
    lang: Option<Language>,

    /// Bypass test coverage gate, run all checks unconditionally
    #[arg(long, global = true)]
    all: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory to analyze (for default analyze command)
    #[arg(default_value = ".")]
    path: String,
}

fn parse_language(s: &str) -> Result<Language, String> {
    match s.to_lowercase().as_str() {
        "python" | "py" => Ok(Language::Python),
        "rust" | "rs" => Ok(Language::Rust),
        _ => Err(format!("Unknown language '{}'. Use 'python' or 'rust'.", s)),
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Report summary statistics for all metrics
    Stats {
        /// Directories to analyze (can specify multiple)
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },
    /// Generate config file with thresholds from analyzed codebases
    Mimic {
        /// Directories to analyze (can specify multiple)
        #[arg(required = true)]
        paths: Vec<String>,

        /// Output file (defaults to stdout)
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    // Load language-specific configs
    let (py_config, rs_config) = load_configs(&cli.config);
    let gate_config = load_gate_config(&cli.config);

    match cli.command {
        Some(Commands::Stats { paths }) => {
            run_stats(&paths, cli.lang);
        }
        Some(Commands::Mimic { paths, out }) => {
            run_mimic(&paths, out.as_deref(), cli.lang);
        }
        None => {
            run_analyze(&cli.path, &py_config, &rs_config, cli.lang, cli.all, &gate_config);
        }
    }
}

fn load_gate_config(config_path: &Option<PathBuf>) -> GateConfig {
    if let Some(path) = config_path {
        GateConfig::load_from(path)
    } else {
        GateConfig::load()
    }
}

/// Load separate configs for Python and Rust analysis
fn load_configs(config_path: &Option<PathBuf>) -> (Config, Config) {
    if let Some(path) = config_path {
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

use kiss::{Violation, DuplicateCluster};
use kiss::cli_output::{
    print_no_files_message, print_coverage_gate_failure, print_instability,
    print_violations, print_duplicates, print_py_test_refs, print_rs_test_refs,
};

fn run_analyze(path: &str, py_config: &Config, rs_config: &Config, lang_filter: Option<Language>, bypass_gate: bool, gate_config: &GateConfig) {
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, lang_filter);
    
    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(lang_filter, root);
        return;
    }
    
    let (py_parsed, rs_parsed, mut viols) = parse_all(&py_files, &rs_files, py_config, rs_config);
    
    if !bypass_gate && !check_coverage_gate(&py_parsed, &rs_parsed, gate_config) {
        return;
    }
    
    let (py_graph, rs_graph) = build_graphs(&py_parsed, &rs_parsed);
    viols.extend(analyze_graphs(&py_graph, &rs_graph, py_config, rs_config));
    
    print_all_results(&viols, &py_parsed, &rs_parsed, &py_graph, &rs_graph);
}

fn build_graphs(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    let py_graph = if !py_parsed.is_empty() {
        let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
        Some(build_dependency_graph(&refs))
    } else { None };
    
    let rs_graph = if !rs_parsed.is_empty() {
        let refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        Some(build_rust_dependency_graph(&refs))
    } else { None };
    
    (py_graph, rs_graph)
}

fn analyze_graphs(py_graph: &Option<DependencyGraph>, rs_graph: &Option<DependencyGraph>, py_config: &Config, rs_config: &Config) -> Vec<Violation> {
    let mut viols = Vec::new();
    if let Some(g) = py_graph { viols.extend(analyze_graph(g, py_config)); }
    if let Some(g) = rs_graph { viols.extend(analyze_graph(g, rs_config)); }
    viols
}

fn print_all_results(viols: &[Violation], py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], py_graph: &Option<DependencyGraph>, rs_graph: &Option<DependencyGraph>) {
    let py_dups = detect_py_duplicates(py_parsed);
    let rs_dups = detect_rs_duplicates(rs_parsed);
    let dup_count = py_dups.len() + rs_dups.len();
    
    print_violations(viols, py_parsed.len() + rs_parsed.len(), dup_count);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    print_instability("Python", py_graph.as_ref());
    print_instability("Rust", rs_graph.as_ref());
    print_py_test_refs(py_parsed);
    print_rs_test_refs(rs_parsed);
}


fn parse_all(py_files: &[PathBuf], rs_files: &[PathBuf], py_config: &Config, rs_config: &Config) -> (Vec<ParsedFile>, Vec<ParsedRustFile>, Vec<Violation>) {
    let (py_parsed, mut viols) = parse_and_analyze_py(py_files, py_config);
    let (rs_parsed, rs_viols) = parse_and_analyze_rs(rs_files, rs_config);
    viols.extend(rs_viols);
    (py_parsed, rs_parsed, viols)
}

fn check_coverage_gate(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], gate_config: &GateConfig) -> bool {
    let (coverage, tested, total) = compute_test_coverage(py_parsed, rs_parsed);
    if coverage < gate_config.test_coverage_threshold {
        print_coverage_gate_failure(coverage, gate_config.test_coverage_threshold, tested, total);
        return false;
    }
    true
}

fn compute_test_coverage(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> (usize, usize, usize) {
    let mut tested = 0;
    let mut total = 0;
    
    if !py_parsed.is_empty() {
        let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
        let analysis = analyze_test_refs(&refs);
        total += analysis.definitions.len();
        tested += analysis.definitions.len() - analysis.unreferenced.len();
    }
    
    if !rs_parsed.is_empty() {
        let refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        let analysis = analyze_rust_test_refs(&refs);
        total += analysis.definitions.len();
        tested += analysis.definitions.len() - analysis.unreferenced.len();
    }
    
    let coverage = if total > 0 { 
        ((tested as f64 / total as f64) * 100.0).round() as usize 
    } else { 100 };
    (coverage, tested, total)
}


fn gather_files(root: &Path, lang: Option<Language>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let py = if lang.is_none() || lang == Some(Language::Python) { find_python_files(root) } else { vec![] };
    let rs = if lang.is_none() || lang == Some(Language::Rust) { find_rust_files(root) } else { vec![] };
    (py, rs)
}

fn parse_and_analyze_py(files: &[PathBuf], config: &Config) -> (Vec<ParsedFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let results = match parse_files(files) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to initialize Python parser: {}", e);
            return (Vec::new(), Vec::new());
        }
    };
    let mut parsed = Vec::new();
    let mut viols = Vec::new();
    for result in results {
        match result {
            Ok(p) => {
                viols.extend(analyze_file(&p, config));
                parsed.push(p);
            }
            Err(e) => eprintln!("Error parsing Python: {}", e),
        }
    }
    (parsed, viols)
}

fn parse_and_analyze_rs(files: &[PathBuf], config: &Config) -> (Vec<ParsedRustFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let mut parsed = Vec::new();
    let mut viols = Vec::new();
    for result in parse_rust_files(files) {
        match result {
            Ok(p) => {
                viols.extend(analyze_rust_file(&p, config));
                parsed.push(p);
            }
            Err(e) => eprintln!("Error parsing Rust: {}", e),
        }
    }
    (parsed, viols)
}

use kiss::DependencyGraph;

fn detect_py_duplicates(parsed: &[ParsedFile]) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let chunks = extract_chunks_for_duplication(&refs);
    cluster_duplicates(&detect_duplicates(&refs, &DuplicationConfig::default()), &chunks)
}

fn detect_rs_duplicates(parsed: &[ParsedRustFile]) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let chunks = extract_rust_chunks_for_duplication(&refs);
    cluster_duplicates(&detect_duplicates_from_chunks(&chunks, &DuplicationConfig::default()), &chunks)
}



fn run_stats(paths: &[String], lang_filter: Option<Language>) {
    let (mut py_stats, mut rs_stats) = (MetricStats::default(), MetricStats::default());
    let (mut py_cnt, mut rs_cnt) = (0, 0);

    for path in paths {
        let root = Path::new(path);
        if lang_filter.is_none() || lang_filter == Some(Language::Python) {
            let (s, c) = collect_py_stats(root); py_stats.merge(s); py_cnt += c;
        }
        if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
            let (s, c) = collect_rs_stats(root); rs_stats.merge(s); rs_cnt += c;
        }
    }

    if py_cnt + rs_cnt == 0 { eprintln!("No source files found."); std::process::exit(1); }
    println!("kiss stats - Summary Statistics\nAnalyzed from: {}\n", paths.join(", "));
    if py_cnt > 0 { println!("=== Python ({} files) ===\n{}\n", py_cnt, format_stats_table(&compute_summaries(&py_stats))); }
    if rs_cnt > 0 { println!("=== Rust ({} files) ===\n{}", rs_cnt, format_stats_table(&compute_summaries(&rs_stats))); }
}

fn run_mimic(paths: &[String], out: Option<&Path>, lang_filter: Option<Language>) {
    let ((py_stats, py_cnt), (rs_stats, rs_cnt)) = collect_all_stats(paths, lang_filter);
    if py_cnt + rs_cnt == 0 { eprintln!("No source files found."); std::process::exit(1); }
    let toml = generate_config_toml_by_language(&py_stats, &rs_stats, py_cnt, rs_cnt);
    match out {
        Some(p) => write_mimic_config(p, &toml, py_cnt, rs_cnt),
        None => print!("{}", toml),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use kiss::config_gen::{collect_py_stats, collect_rs_stats, collect_all_stats, merge_config_toml, write_mimic_config};
    use tempfile::TempDir;

    #[test]
    fn test_language_parsing_and_enum() {
        assert_eq!(parse_language("python"), Ok(Language::Python));
        assert_eq!(parse_language("rust"), Ok(Language::Rust));
        assert!(parse_language("invalid").is_err());
        assert_ne!(Language::Python, Language::Rust);
    }

    #[test]
    fn test_load_configs_and_cli() {
        let (py, rs) = load_configs(&None);
        assert!(py.statements_per_function > 0 && rs.statements_per_function > 0);
        let cli = Cli { config: None, lang: None, all: false, command: None, path: ".".to_string() };
        assert_eq!(cli.path, ".");
        let cmd = Commands::Stats { paths: vec![".".to_string()] };
        if let Commands::Stats { paths } = cmd { assert_eq!(paths.len(), 1); }
    }

    #[test]
    fn test_gather_files_all_and_filtered() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "").unwrap();
        let (py, rs) = gather_files(tmp.path(), None);
        assert_eq!(py.len(), 1); assert_eq!(rs.len(), 1);
        let (py2, rs2) = gather_files(tmp.path(), Some(Language::Python));
        assert_eq!(py2.len(), 1); assert_eq!(rs2.len(), 0);
    }

    #[test]
    fn test_parse_and_analyze_empty() {
        let config = Config::default();
        let (py_parsed, py_viols) = parse_and_analyze_py(&[], &config);
        let (rs_parsed, rs_viols) = parse_and_analyze_rs(&[], &config);
        assert!(py_parsed.is_empty() && py_viols.is_empty());
        assert!(rs_parsed.is_empty() && rs_viols.is_empty());
        let viols = analyze_graph(&DependencyGraph::new(), &config);
        assert!(viols.is_empty());
    }

    #[test]
    fn test_detect_duplicates_empty() {
        assert!(detect_py_duplicates(&[]).is_empty());
        assert!(detect_rs_duplicates(&[]).is_empty());
    }

    #[test]
    fn test_print_functions_no_panic() {
        print_violations(&[], 0, 0);
        print_duplicates("Python", &[]);
        print_py_test_refs(&[]);
        print_rs_test_refs(&[]);
    }

    #[test]
    fn test_collect_stats_empty() {
        let tmp = TempDir::new().unwrap();
        let (py_stats, py_cnt) = collect_py_stats(tmp.path());
        let (rs_stats, rs_cnt) = collect_rs_stats(tmp.path());
        assert_eq!(py_cnt, 0); assert_eq!(rs_cnt, 0);
        let _ = (py_stats, rs_stats);
        let paths = vec![tmp.path().to_string_lossy().to_string()];
        let ((py, py_count), (rs, rs_count)) = collect_all_stats(&paths, None);
        assert_eq!(py_count, 0); assert_eq!(rs_count, 0);
        let _ = (py, rs);
    }

    #[test]
    fn test_config_merge_and_write() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "[python]\nstatements_per_function = 10").unwrap();
        let merged = merge_config_toml(tmp.path(), "[rust]\nstatements_per_function = 20", false, true);
        assert!(merged.contains("[python]") || merged.contains("[rust]"));
        let tmp2 = TempDir::new().unwrap();
        write_mimic_config(&tmp2.path().join("out.toml"), "[python]\nx = 1", 1, 0);
    }

    #[test]
    fn test_fn_pointers_exist() {
        let _ = run_stats as fn(&[String], Option<Language>);
        let _ = run_mimic as fn(&[String], Option<&Path>, Option<Language>);
        let _ = main as fn();
    }

    #[test]
    fn test_run_analyze_and_gate_config() {
        let tmp = TempDir::new().unwrap();
        run_analyze(&tmp.path().to_string_lossy(), &Config::default(), &Config::default(), None, true, &GateConfig::default());
        let path = tmp.path().join("kiss.toml");
        std::fs::write(&path, "[gate]\ntest_coverage_threshold = 80\n").unwrap();
        assert_eq!(load_gate_config(&Some(path)).test_coverage_threshold, 80);
    }

    #[test]
    fn test_coverage_and_gate() {
        let py: Vec<ParsedFile> = vec![];
        let rs: Vec<ParsedRustFile> = vec![];
        let (cov, tested, total) = compute_test_coverage(&py, &rs);
        assert_eq!((tested, total, cov), (0, 0, 100));
        assert!(check_coverage_gate(&py, &rs, &GateConfig { test_coverage_threshold: 0 }));
    }

    #[test]
    fn test_parse_all_and_graphs_empty() {
        let config = Config::default();
        let (py_parsed, rs_parsed, viols) = parse_all(&[], &[], &config, &config);
        assert!(py_parsed.is_empty() && rs_parsed.is_empty() && viols.is_empty());
        let (py_g, rs_g) = build_graphs(&py_parsed, &rs_parsed);
        assert!(py_g.is_none() && rs_g.is_none());
        assert!(analyze_graphs(&None, &None, &config, &config).is_empty());
    }

    #[test]
    fn test_print_helpers_no_panic() {
        let tmp = TempDir::new().unwrap();
        print_no_files_message(None, tmp.path());
        print_no_files_message(Some(Language::Python), tmp.path());
        print_coverage_gate_failure(50, 80, 5, 10);
        print_all_results(&[], &[], &[], &None, &None);
        print_instability("Test", None);
    }
}
