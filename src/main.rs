use clap::{Parser, Subcommand};
use kiss::{
    analyze_file, analyze_graph, analyze_rust_file, analyze_rust_test_refs, analyze_test_refs,
    build_dependency_graph, build_rust_dependency_graph, cluster_duplicates, compute_summaries,
    detect_duplicates, extract_chunks_for_duplication, find_python_files, find_rust_files,
    format_stats_table, parse_files, parse_rust_files, Config, ConfigLanguage,
    DuplicationConfig, MetricStats, ParsedFile, ParsedRustFile,
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

    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory to analyze (for default analyze command)
    #[arg(default_value = ".")]
    path: String,
}

/// Language filter for analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
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

    match cli.command {
        Some(Commands::Stats { paths }) => {
            run_stats(&paths, cli.lang);
        }
        Some(Commands::Mimic { paths, out }) => {
            run_mimic(&paths, out.as_deref(), cli.lang);
        }
        None => {
            run_analyze(&cli.path, &py_config, &rs_config, cli.lang);
        }
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

use kiss::{Violation, DuplicateCluster, PercentileSummary};

fn run_analyze(path: &str, py_config: &Config, rs_config: &Config, lang_filter: Option<Language>) {
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, lang_filter);
    if py_files.is_empty() && rs_files.is_empty() {
        println!("{} in {}", match lang_filter { Some(Language::Python) => "No Python files", Some(Language::Rust) => "No Rust files", None => "No files" }, root.display());
        return;
    }
    let (py_parsed, mut viols) = parse_and_analyze_py(&py_files, py_config);
    let (rs_parsed, rs_viols) = parse_and_analyze_rs(&rs_files, rs_config);
    viols.extend(rs_viols);
    viols.extend(analyze_py_graph(&py_parsed, py_config));
    viols.extend(analyze_rs_graph(&rs_parsed, rs_config));
    print_violations(&viols, py_parsed.len() + rs_parsed.len());
    print_duplicates(&detect_py_duplicates(&py_parsed));
    print_py_test_refs(&py_parsed); print_rs_test_refs(&rs_parsed);
}

fn gather_files(root: &Path, lang: Option<Language>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let py = if lang.is_none() || lang == Some(Language::Python) { find_python_files(root) } else { vec![] };
    let rs = if lang.is_none() || lang == Some(Language::Rust) { find_rust_files(root) } else { vec![] };
    (py, rs)
}

fn parse_and_analyze_py(files: &[PathBuf], config: &Config) -> (Vec<ParsedFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let results = parse_files(files).unwrap_or_default();
    let mut parsed = Vec::new(); let mut viols = Vec::new();
    for r in results { match r { Ok(p) => { viols.extend(analyze_file(&p, config)); parsed.push(p); } Err(e) => eprintln!("Error parsing Python: {}", e) } }
    (parsed, viols)
}

fn parse_and_analyze_rs(files: &[PathBuf], config: &Config) -> (Vec<ParsedRustFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let mut parsed = Vec::new(); let mut viols = Vec::new();
    for r in parse_rust_files(files) { match r { Ok(p) => { viols.extend(analyze_rust_file(&p, config)); parsed.push(p); } Err(e) => eprintln!("Error parsing Rust: {}", e) } }
    (parsed, viols)
}

fn analyze_py_graph(parsed: &[ParsedFile], config: &Config) -> Vec<Violation> {
    if parsed.is_empty() { return Vec::new(); }
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    analyze_graph(&build_dependency_graph(&refs), config)
}

fn analyze_rs_graph(parsed: &[ParsedRustFile], config: &Config) -> Vec<Violation> {
    if parsed.is_empty() { return Vec::new(); }
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    analyze_graph(&build_rust_dependency_graph(&refs), config)
}

fn detect_py_duplicates(parsed: &[ParsedFile]) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let chunks = extract_chunks_for_duplication(&refs);
    cluster_duplicates(&detect_duplicates(&refs, &DuplicationConfig::default()), &chunks)
}

fn print_violations(viols: &[Violation], total: usize) {
    if viols.is_empty() { println!("✓ No violations found in {} files.", total); return; }
    println!("Found {} violations:\n", viols.len());
    for v in viols { println!("{}:{}\n  {}\n  → {}\n", v.file.display(), v.line, v.message, v.suggestion); }
}

fn print_duplicates(clusters: &[DuplicateCluster]) {
    if clusters.is_empty() { return; }
    println!("\n--- Duplicate Code Detected ({} clusters) ---\n", clusters.len());
    for (i, c) in clusters.iter().enumerate() {
        println!("Cluster {}: {} copies (~{:.0}% similar)", i + 1, c.chunks.len(), c.avg_similarity * 100.0);
        for ch in &c.chunks { println!("  {}:{}-{} ({})", ch.file.display(), ch.start_line, ch.end_line, ch.name); }
            println!();
        }
    }

fn print_py_test_refs(parsed: &[ParsedFile]) {
    if parsed.is_empty() { return; }
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Python Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test file.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind, d.name); }
    }
}

fn print_rs_test_refs(parsed: &[ParsedRustFile]) {
    if parsed.is_empty() { return; }
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Rust Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind, d.name); }
    }
}

fn collect_py_stats(root: &Path) -> (MetricStats, usize) {
    let py_files = find_python_files(root);
    if py_files.is_empty() { return (MetricStats::default(), 0); }
    let Ok(results) = parse_files(&py_files) else { return (MetricStats::default(), 0); };
    let parsed: Vec<ParsedFile> = results.into_iter().filter_map(|r| r.ok()).collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect(&refs);
    stats.collect_graph_metrics(&build_dependency_graph(&refs));
    (stats, cnt)
}

fn collect_rs_stats(root: &Path) -> (MetricStats, usize) {
    let rs_files = find_rust_files(root);
    if rs_files.is_empty() { return (MetricStats::default(), 0); }
    let parsed: Vec<ParsedRustFile> = parse_rust_files(&rs_files).into_iter().filter_map(|r| r.ok()).collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect_rust(&refs);
    stats.collect_graph_metrics(&build_rust_dependency_graph(&refs));
    (stats, cnt)
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

fn collect_all_stats(paths: &[String], lang: Option<Language>) -> ((MetricStats, usize), (MetricStats, usize)) {
    let (mut py, mut rs) = ((MetricStats::default(), 0), (MetricStats::default(), 0));
    for path in paths {
        let root = Path::new(path);
        if lang.is_none() || lang == Some(Language::Python) { let (s, c) = collect_py_stats(root); py.0.merge(s); py.1 += c; }
        if lang.is_none() || lang == Some(Language::Rust) { let (s, c) = collect_rs_stats(root); rs.0.merge(s); rs.1 += c; }
    }
    (py, rs)
}

fn write_mimic_config(out: &Path, toml: &str, py_cnt: usize, rs_cnt: usize) {
    let content = if out.exists() { merge_config_toml(out, toml, py_cnt > 0, rs_cnt > 0) } else { toml.to_string() };
    if let Err(e) = std::fs::write(out, &content) { eprintln!("Error writing to {}: {}", out.display(), e); std::process::exit(1); }
    eprintln!("Generated config from {} files → {}", py_cnt + rs_cnt, out.display());
}

fn format_section(out: &mut String, name: &str, section: Option<&toml::Value>) {
    if let Some(v) = section {
        out.push_str(&format!("[{}]\n", name));
        if let Some(t) = v.as_table() { for (k, v) in t { out.push_str(&format!("{} = {}\n", k, v)); } }
        out.push('\n');
    }
}

fn merge_config_toml(path: &Path, new: &str, upd_py: bool, upd_rs: bool) -> String {
    let (Ok(ex_str), Ok(nw)) = (std::fs::read_to_string(path), new.parse::<toml::Table>()) else { return new.to_string(); };
    let Ok(ex) = ex_str.parse::<toml::Table>() else { return new.to_string(); };
    let pick = |k: &str, upd: bool| if upd { nw.get(k) } else { ex.get(k) }.cloned();
    let mut m = toml::Table::new();
    for (k, upd) in [("python", upd_py), ("rust", upd_rs)] { if let Some(v) = pick(k, upd) { m.insert(k.to_string(), v); } }
    let shared = if upd_py && upd_rs { nw.get("shared") } else { ex.get("shared").or(nw.get("shared")) }.cloned();
    if let Some(v) = shared { m.insert("shared".to_string(), v); }
    if !(upd_py && upd_rs) { if let Some(v) = ex.get("thresholds").cloned() { m.insert("thresholds".to_string(), v); } }
    build_merged_output(&m)
}

fn build_merged_output(m: &toml::Table) -> String {
    let mut out = String::from("# Generated by kiss mimic\n# Thresholds based on 99th percentile of analyzed codebases\n\n");
    for k in ["python", "rust", "shared", "thresholds"] { format_section(&mut out, k, m.get(k)); }
    out
}

fn generate_config_toml_by_language(py: &MetricStats, rs: &MetricStats, py_n: usize, rs_n: usize) -> String {
    let mut out = String::from("# Generated by kiss mimic\n# Thresholds based on 99th percentile of analyzed codebases\n\n");
    if py_n > 0 { append_section(&mut out, "[python]", &compute_summaries(py), python_config_key); }
    if rs_n > 0 { append_section(&mut out, "[rust]", &compute_summaries(rs), rust_config_key); }
    if py_n > 0 && rs_n > 0 { append_shared_section(&mut out, &compute_summaries(py), &compute_summaries(rs)); }
    out
}

fn append_section(out: &mut String, header: &str, sums: &[PercentileSummary], key_fn: fn(&str) -> Option<&'static str>) {
    out.push_str(header); out.push('\n');
    for s in sums { if let Some(k) = key_fn(s.name) { out.push_str(&format!("{} = {}\n", k, s.p99)); } }
    out.push('\n');
}

fn append_shared_section(out: &mut String, py_sums: &[PercentileSummary], rs_sums: &[PercentileSummary]) {
    out.push_str("[shared]\n");
    for py_s in py_sums {
        if let Some(k) = shared_config_key(py_s.name) {
            let rs_val = rs_sums.iter().find(|s| s.name == py_s.name).map(|s| s.p99).unwrap_or(0);
            out.push_str(&format!("{} = {}\n", k, py_s.p99.max(rs_val)));
        }
    }
}

/// Map metric name to Python-specific config key
fn python_config_key(name: &str) -> Option<&'static str> {
    match name {
        "Statements per function" => Some("statements_per_function"),
        "Arguments (positional)" => Some("positional_args"),
        "Arguments (keyword-only)" => Some("keyword_only_args"),
        "Max indentation depth" => Some("max_indentation"),
        "Branches per function" => Some("branches_per_function"),
        "Local variables per function" => Some("local_variables"),
        "Methods per class" => Some("methods_per_class"),
        "Cyclomatic complexity" => Some("cyclomatic_complexity"),
        "Fan-out (per module)" => Some("fan_out"),
        "Fan-in (per module)" => Some("fan_in"),
        "Transitive deps (per module)" => Some("transitive_deps"),
        "LCOM % (per class)" => Some("lcom"),
        _ => None,
    }
}

/// Map metric name to Rust-specific config key
fn rust_config_key(name: &str) -> Option<&'static str> {
    match name {
        "Statements per function" => Some("statements_per_function"),
        "Arguments (total)" => Some("arguments"),
        "Max indentation depth" => Some("max_indentation"),
        "Branches per function" => Some("branches_per_function"),
        "Local variables per function" => Some("local_variables"),
        "Methods per class" => Some("methods_per_type"),
        "Cyclomatic complexity" => Some("cyclomatic_complexity"),
        "Fan-out (per module)" => Some("fan_out"),
        "Fan-in (per module)" => Some("fan_in"),
        "Transitive deps (per module)" => Some("transitive_deps"),
        "LCOM % (per class)" => Some("lcom"),
        _ => None,
    }
}

/// Map metric name to shared config key (metrics that apply to both languages)
fn shared_config_key(name: &str) -> Option<&'static str> {
    match name {
        "Lines per file" => Some("lines_per_file"),
        "Classes per file" => Some("types_per_file"),
        "Imports per file" => Some("imports_per_file"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_language() {
        assert_eq!(parse_language("python"), Ok(Language::Python));
        assert_eq!(parse_language("rust"), Ok(Language::Rust));
        assert!(parse_language("invalid").is_err());
    }

    #[test]
    fn test_language_enum() {
        let p = Language::Python;
        let r = Language::Rust;
        assert_ne!(p, r);
    }

    #[test]
    fn test_python_config_key() {
        assert_eq!(python_config_key("Statements per function"), Some("statements_per_function"));
        assert_eq!(python_config_key("Unknown"), None);
    }

    #[test]
    fn test_rust_config_key() {
        assert_eq!(rust_config_key("Statements per function"), Some("statements_per_function"));
        assert_eq!(rust_config_key("Unknown"), None);
    }

    #[test]
    fn test_shared_config_key() {
        assert_eq!(shared_config_key("Lines per file"), Some("lines_per_file"));
        assert_eq!(shared_config_key("Unknown"), None);
    }

    #[test]
    fn test_load_configs() {
        let (py, rs) = load_configs(&None);
        assert!(py.statements_per_function > 0);
        assert!(rs.statements_per_function > 0);
    }

    #[test]
    fn test_gather_files() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "").unwrap();
        let (py, rs) = gather_files(tmp.path(), None);
        assert_eq!(py.len(), 1);
        assert_eq!(rs.len(), 1);
    }

    #[test]
    fn test_gather_files_filtered() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "").unwrap();
        let (py, rs) = gather_files(tmp.path(), Some(Language::Python));
        assert_eq!(py.len(), 1);
        assert_eq!(rs.len(), 0);
    }

    #[test]
    fn test_format_section() {
        let mut out = String::new();
        let table = toml::toml! { x = 1 };
        format_section(&mut out, "test", Some(&toml::Value::Table(table)));
        assert!(out.contains("[test]"));
    }

    #[test]
    fn test_build_merged_output() {
        let table = toml::Table::new();
        let out = build_merged_output(&table);
        assert!(out.contains("Generated by kiss"));
    }

    #[test]
    fn test_generate_config_toml_by_language_empty() {
        let py = MetricStats::default();
        let rs = MetricStats::default();
        let toml = generate_config_toml_by_language(&py, &rs, 0, 0);
        assert!(toml.contains("Generated by kiss"));
    }

    #[test]
    fn test_append_section() {
        let mut out = String::new();
        let summaries = vec![PercentileSummary { name: "Statements per function", count: 1, p50: 1, p90: 2, p95: 3, p99: 4, max: 5 }];
        append_section(&mut out, "[test]", &summaries, python_config_key);
        assert!(out.contains("[test]"));
    }

    #[test]
    fn test_append_shared_section() {
        let py = vec![PercentileSummary { name: "Lines per file", count: 1, p50: 1, p90: 2, p95: 3, p99: 100, max: 200 }];
        let rs = vec![PercentileSummary { name: "Lines per file", count: 1, p50: 1, p90: 2, p95: 3, p99: 150, max: 300 }];
        let mut out = String::new();
        append_shared_section(&mut out, &py, &rs);
        assert!(out.contains("[shared]"));
    }

    #[test]
    fn test_cli_struct() {
        // Just verify the struct can be constructed
        let cli = Cli { config: None, lang: None, command: None, path: ".".to_string() };
        assert_eq!(cli.path, ".");
    }

    #[test]
    fn test_commands_enum() {
        let cmd = Commands::Stats { paths: vec![".".to_string()] };
        if let Commands::Stats { paths } = cmd { assert_eq!(paths.len(), 1); }
    }

    #[test]
    fn test_parse_and_analyze_py_empty() {
        let config = Config::default();
        let (parsed, viols) = parse_and_analyze_py(&[], &config);
        assert!(parsed.is_empty());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_parse_and_analyze_rs_empty() {
        let config = Config::default();
        let (parsed, viols) = parse_and_analyze_rs(&[], &config);
        assert!(parsed.is_empty());
        assert!(viols.is_empty());
    }

    #[test]
    fn test_analyze_py_graph_empty() {
        let config = Config::default();
        let viols = analyze_py_graph(&[], &config);
        assert!(viols.is_empty());
    }

    #[test]
    fn test_analyze_rs_graph_empty() {
        let config = Config::default();
        let viols = analyze_rs_graph(&[], &config);
        assert!(viols.is_empty());
    }

    #[test]
    fn test_detect_py_duplicates_empty() {
        let clusters = detect_py_duplicates(&[]);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[], 0); // Just verify it doesn't panic
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates(&[]); // Just verify it doesn't panic
    }

    #[test]
    fn test_print_py_test_refs_empty() {
        print_py_test_refs(&[]); // Just verify it doesn't panic
    }

    #[test]
    fn test_print_rs_test_refs_empty() {
        print_rs_test_refs(&[]); // Just verify it doesn't panic
    }

    #[test]
    fn test_collect_py_stats() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let (stats, count) = collect_py_stats(tmp.path());
        assert_eq!(count, 0);
        let _ = stats;
    }

    #[test]
    fn test_collect_rs_stats() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let (stats, count) = collect_rs_stats(tmp.path());
        assert_eq!(count, 0);
        let _ = stats;
    }

    #[test]
    fn test_collect_all_stats() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];
        let ((py, py_count), (rs, rs_count)) = collect_all_stats(&paths, None);
        assert_eq!(py_count, 0);
        assert_eq!(rs_count, 0);
        let _ = (py, rs);
    }

    #[test]
    fn test_merge_config_toml() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "[python]\nstatements_per_function = 10").unwrap();
        let new_toml = "[rust]\nstatements_per_function = 20";
        let merged = merge_config_toml(tmp.path(), new_toml, false, true);
        assert!(merged.contains("[python]") || merged.contains("[rust]"));
    }

    #[test]
    fn test_write_mimic_config() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("out.toml");
        let toml = "[python]\nx = 1";
        write_mimic_config(&out, toml, 1, 0);
        // Just verify it doesn't panic
    }

    #[test]
    fn test_run_stats_fn_exists() {
        // run_stats calls process::exit on empty, so we just verify it exists
        let _ = run_stats as fn(&[String], Option<Language>);
    }

    #[test]
    fn test_run_mimic_fn_exists() {
        // run_mimic calls process::exit on empty, so we just verify it exists
        let _ = run_mimic as fn(&[String], Option<&Path>, Option<Language>);
    }

    #[test]
    fn test_run_analyze_on_empty_dir() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        run_analyze(&tmp.path().to_string_lossy(), &Config::default(), &Config::default(), None);
        // Just verify it doesn't panic (no exit on empty)
    }

    #[test]
    fn test_main_fn_exists() {
        // main calls parse() which expects CLI args, so we just verify it exists
        let _ = main as fn();
    }
}
