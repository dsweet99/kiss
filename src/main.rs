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

fn run_analyze(path: &str, py_config: &Config, rs_config: &Config, lang_filter: Option<Language>) {
    let root = Path::new(path);
    let py_files = if lang_filter.is_none() || lang_filter == Some(Language::Python) {
        find_python_files(root)
    } else {
        Vec::new()
    };
    let rs_files = if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
        find_rust_files(root)
    } else {
        Vec::new()
    };

    if py_files.is_empty() && rs_files.is_empty() {
        let msg = match lang_filter {
            Some(Language::Python) => "No Python files found",
            Some(Language::Rust) => "No Rust files found",
            None => "No Python or Rust files found",
        };
        println!("{} in {}", msg, root.display());
        return;
    }

    let mut all_violations = Vec::new();
    let mut total_files = 0;

    // Process Python files with Python config
    let mut parsed_py_files: Vec<ParsedFile> = Vec::new();
    if !py_files.is_empty() {
        let results = match parse_files(&py_files) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {}", e);
                Vec::new()
            }
        };

        for result in results {
            match result {
                Ok(parsed) => {
                    let violations = analyze_file(&parsed, py_config);
                    all_violations.extend(violations);
                    parsed_py_files.push(parsed);
                }
                Err(e) => {
                    eprintln!("Error parsing Python file: {}", e);
                }
            }
        }
        total_files += parsed_py_files.len();
    }

    // Process Rust files with Rust config
    let mut parsed_rs_files: Vec<ParsedRustFile> = Vec::new();
    if !rs_files.is_empty() {
        let results = parse_rust_files(&rs_files);
        for result in results {
            match result {
                Ok(parsed) => {
                    let violations = analyze_rust_file(&parsed, rs_config);
                    all_violations.extend(violations);
                    parsed_rs_files.push(parsed);
                }
                Err(e) => {
                    eprintln!("Error parsing Rust file: {}", e);
                }
            }
        }
        total_files += parsed_rs_files.len();
    }

    // Analyze Python dependency graph with Python config
    if !parsed_py_files.is_empty() {
        let parsed_refs: Vec<&ParsedFile> = parsed_py_files.iter().collect();
        let dep_graph = build_dependency_graph(&parsed_refs);
        let graph_violations = analyze_graph(&dep_graph, py_config);
        all_violations.extend(graph_violations);
    }

    // Analyze Rust dependency graph with Rust config
    if !parsed_rs_files.is_empty() {
        let parsed_refs: Vec<&ParsedRustFile> = parsed_rs_files.iter().collect();
        let dep_graph = build_rust_dependency_graph(&parsed_refs);
        let graph_violations = analyze_graph(&dep_graph, rs_config);
        all_violations.extend(graph_violations);
    }

    // Detect duplicates in Python files (duplication is text-based)
    let parsed_py_refs: Vec<&ParsedFile> = parsed_py_files.iter().collect();
    let dup_config = DuplicationConfig::default();
    let chunks = extract_chunks_for_duplication(&parsed_py_refs);
    let pairs = detect_duplicates(&parsed_py_refs, &dup_config);
    let clusters = cluster_duplicates(&pairs, &chunks);

    // Report violations
    if all_violations.is_empty() {
        println!("✓ No violations found in {} files.", total_files);
    } else {
        println!("Found {} violations:\n", all_violations.len());

        for v in &all_violations {
            println!("{}:{}", v.file.display(), v.line);
            println!("  {}", v.message);
            println!("  → {}\n", v.suggestion);
        }
    }

    // Report duplicate clusters
    if !clusters.is_empty() {
        println!(
            "\n--- Duplicate Code Detected ({} clusters) ---\n",
            clusters.len()
        );

        for (i, cluster) in clusters.iter().enumerate() {
            println!(
                "Cluster {}: {} copies (~{:.0}% similar)",
                i + 1,
                cluster.chunks.len(),
                cluster.avg_similarity * 100.0
            );
            for chunk in &cluster.chunks {
                println!(
                    "  {}:{}-{} ({})",
                    chunk.file.display(),
                    chunk.start_line,
                    chunk.end_line,
                    chunk.name
                );
            }
            println!();
        }
    }

    // Analyze Python test references
    if !parsed_py_files.is_empty() {
        let parsed_refs: Vec<&ParsedFile> = parsed_py_files.iter().collect();
        let test_analysis = analyze_test_refs(&parsed_refs);
        if !test_analysis.unreferenced.is_empty() {
            println!(
                "\n--- Possibly Untested Python Code ({} items) ---\n",
                test_analysis.unreferenced.len()
            );
            println!("The following code units are not referenced by any test file.");
            println!("(Note: This is static analysis only; actual coverage may differ.)\n");

            for def in &test_analysis.unreferenced {
                println!("  {}:{} {} '{}'", def.file.display(), def.line, def.kind, def.name);
            }
        }
    }

    // Analyze Rust test references
    if !parsed_rs_files.is_empty() {
        let parsed_refs: Vec<&ParsedRustFile> = parsed_rs_files.iter().collect();
        let test_analysis = analyze_rust_test_refs(&parsed_refs);
        if !test_analysis.unreferenced.is_empty() {
            println!(
                "\n--- Possibly Untested Rust Code ({} items) ---\n",
                test_analysis.unreferenced.len()
            );
            println!("The following code units are not referenced by any test.");
            println!("(Note: This is static analysis only; actual coverage may differ.)\n");

            for def in &test_analysis.unreferenced {
                println!("  {}:{} {} '{}'", def.file.display(), def.line, def.kind, def.name);
            }
        }
    }
}

fn run_stats(paths: &[String], lang_filter: Option<Language>) {
    let mut py_stats = MetricStats::default();
    let mut rs_stats = MetricStats::default();
    let mut py_file_count = 0;
    let mut rs_file_count = 0;

    for path in paths {
        let root = Path::new(path);

        // Process Python files
        if lang_filter.is_none() || lang_filter == Some(Language::Python) {
            let py_files = find_python_files(root);
            if !py_files.is_empty() {
                if let Ok(results) = parse_files(&py_files) {
                    let parsed_files: Vec<ParsedFile> = results.into_iter().filter_map(|r| r.ok()).collect();
                    py_file_count += parsed_files.len();

                    let parsed_refs: Vec<&ParsedFile> = parsed_files.iter().collect();
                    let mut stats = MetricStats::collect(&parsed_refs);

                    let dep_graph = build_dependency_graph(&parsed_refs);
                    stats.collect_graph_metrics(&dep_graph);

                    py_stats.merge(stats);
                }
            }
        }

        // Process Rust files
        if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
            let rs_files = find_rust_files(root);
            if !rs_files.is_empty() {
                let results = parse_rust_files(&rs_files);
                let parsed_files: Vec<ParsedRustFile> = results.into_iter().filter_map(|r| r.ok()).collect();
                rs_file_count += parsed_files.len();

                let parsed_refs: Vec<&ParsedRustFile> = parsed_files.iter().collect();
                let mut stats = MetricStats::collect_rust(&parsed_refs);

                let dep_graph = build_rust_dependency_graph(&parsed_refs);
                stats.collect_graph_metrics(&dep_graph);

                rs_stats.merge(stats);
            }
        }
    }

    let total_files = py_file_count + rs_file_count;
    if total_files == 0 {
        eprintln!("No source files found in any of the specified paths.");
        std::process::exit(1);
    }

    println!("kiss stats - Summary Statistics");
    println!("Analyzed from: {}\n", paths.join(", "));

    // Print Python stats if we have any
    if py_file_count > 0 {
        println!("=== Python ({} files) ===\n", py_file_count);
        let summaries = compute_summaries(&py_stats);
        print!("{}", format_stats_table(&summaries));
        println!();
    }

    // Print Rust stats if we have any
    if rs_file_count > 0 {
        println!("=== Rust ({} files) ===\n", rs_file_count);
        let summaries = compute_summaries(&rs_stats);
        print!("{}", format_stats_table(&summaries));
    }
}

fn run_mimic(paths: &[String], out: Option<&Path>, lang_filter: Option<Language>) {
    let mut py_stats = MetricStats::default();
    let mut rs_stats = MetricStats::default();
    let mut py_file_count = 0;
    let mut rs_file_count = 0;

    for path in paths {
        let root = Path::new(path);

        // Process Python files
        if lang_filter.is_none() || lang_filter == Some(Language::Python) {
            let py_files = find_python_files(root);
            if !py_files.is_empty() {
                if let Ok(results) = parse_files(&py_files) {
                    let parsed_files: Vec<ParsedFile> = results.into_iter().filter_map(|r| r.ok()).collect();
                    py_file_count += parsed_files.len();

                    let parsed_refs: Vec<&ParsedFile> = parsed_files.iter().collect();
                    let mut stats = MetricStats::collect(&parsed_refs);

                    let dep_graph = build_dependency_graph(&parsed_refs);
                    stats.collect_graph_metrics(&dep_graph);

                    py_stats.merge(stats);
                }
            }
        }

        // Process Rust files
        if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
            let rs_files = find_rust_files(root);
            if !rs_files.is_empty() {
                let results = parse_rust_files(&rs_files);
                let parsed_files: Vec<ParsedRustFile> = results.into_iter().filter_map(|r| r.ok()).collect();
                rs_file_count += parsed_files.len();

                let parsed_refs: Vec<&ParsedRustFile> = parsed_files.iter().collect();
                let mut stats = MetricStats::collect_rust(&parsed_refs);

                let dep_graph = build_rust_dependency_graph(&parsed_refs);
                stats.collect_graph_metrics(&dep_graph);

                rs_stats.merge(stats);
            }
        }
    }

    let total_files = py_file_count + rs_file_count;
    if total_files == 0 {
        eprintln!("No source files found in any of the specified paths.");
        std::process::exit(1);
    }

    // Generate config with language-specific sections
    let config_toml = generate_config_toml_by_language(&py_stats, &rs_stats, py_file_count, rs_file_count);

    if let Some(out_path) = out {
        // Merge mode: preserve existing sections not being updated
        let final_toml = if out_path.exists() {
            merge_config_toml(out_path, &config_toml, py_file_count > 0, rs_file_count > 0)
        } else {
            config_toml
        };

        match std::fs::write(out_path, &final_toml) {
            Ok(()) => {
                eprintln!(
                    "Generated config from {} files → {}",
                    total_files,
                    out_path.display()
                );
            }
            Err(e) => {
                eprintln!("Error writing to {}: {}", out_path.display(), e);
                std::process::exit(1);
            }
        }
    } else {
        // Output to stdout
        print!("{}", config_toml);
    }
}

/// Merge new config sections with existing file, preserving sections not being updated
fn merge_config_toml(existing_path: &Path, new_content: &str, updating_python: bool, updating_rust: bool) -> String {
    let existing_content = match std::fs::read_to_string(existing_path) {
        Ok(c) => c,
        Err(_) => return new_content.to_string(),
    };

    let Ok(existing_table) = existing_content.parse::<toml::Table>() else {
        return new_content.to_string();
    };

    let Ok(new_table) = new_content.parse::<toml::Table>() else {
        return new_content.to_string();
    };

    let mut merged = toml::Table::new();

    // Start with header comment
    let mut output = String::new();
    output.push_str("# Generated by kiss mimic\n");
    output.push_str("# Thresholds based on 99th percentile of analyzed codebases\n\n");

    // Python section: use new if updating, else preserve existing
    let python_section = if updating_python {
        new_table.get("python").cloned()
    } else {
        existing_table.get("python").cloned()
    };
    if let Some(section) = python_section {
        merged.insert("python".to_string(), section);
    }

    // Rust section: use new if updating, else preserve existing
    let rust_section = if updating_rust {
        new_table.get("rust").cloned()
    } else {
        existing_table.get("rust").cloned()
    };
    if let Some(section) = rust_section {
        merged.insert("rust".to_string(), section);
    }

    // Shared section: use new if both updating, else use existing, else use new if present
    let shared_section = if updating_python && updating_rust {
        new_table.get("shared").cloned()
    } else {
        existing_table.get("shared").cloned().or_else(|| new_table.get("shared").cloned())
    };
    if let Some(section) = shared_section {
        merged.insert("shared".to_string(), section);
    }

    // Preserve legacy [thresholds] section if it exists and we're not updating everything
    if !(updating_python && updating_rust) {
        if let Some(thresholds) = existing_table.get("thresholds").cloned() {
            merged.insert("thresholds".to_string(), thresholds);
        }
    }

    // Format output nicely
    if let Some(py) = merged.get("python") {
        output.push_str("[python]\n");
        if let Some(table) = py.as_table() {
            for (k, v) in table {
                output.push_str(&format!("{} = {}\n", k, v));
            }
        }
        output.push('\n');
    }

    if let Some(rs) = merged.get("rust") {
        output.push_str("[rust]\n");
        if let Some(table) = rs.as_table() {
            for (k, v) in table {
                output.push_str(&format!("{} = {}\n", k, v));
            }
        }
        output.push('\n');
    }

    if let Some(shared) = merged.get("shared") {
        output.push_str("[shared]\n");
        if let Some(table) = shared.as_table() {
            for (k, v) in table {
                output.push_str(&format!("{} = {}\n", k, v));
            }
        }
        output.push('\n');
    }

    if let Some(thresholds) = merged.get("thresholds") {
        output.push_str("[thresholds]\n");
        if let Some(table) = thresholds.as_table() {
            for (k, v) in table {
                output.push_str(&format!("{} = {}\n", k, v));
            }
        }
        output.push('\n');
    }

    output
}

/// Generate config TOML with language-specific sections
fn generate_config_toml_by_language(
    py_stats: &MetricStats,
    rs_stats: &MetricStats,
    py_file_count: usize,
    rs_file_count: usize,
) -> String {
    let mut output = String::new();
    output.push_str("# Generated by kiss mimic\n");
    output.push_str("# Thresholds based on 99th percentile of analyzed codebases\n\n");

    // Python section
    if py_file_count > 0 {
        output.push_str("[python]\n");
        let summaries = compute_summaries(py_stats);
        for s in &summaries {
            if let Some(key) = python_config_key(s.name) {
                output.push_str(&format!("{} = {}\n", key, s.p99));
            }
        }
        output.push('\n');
    }

    // Rust section
    if rs_file_count > 0 {
        output.push_str("[rust]\n");
        let summaries = compute_summaries(rs_stats);
        for s in &summaries {
            if let Some(key) = rust_config_key(s.name) {
                output.push_str(&format!("{} = {}\n", key, s.p99));
            }
        }
        output.push('\n');
    }

    // Shared section (metrics that are the same for both languages)
    if py_file_count > 0 && rs_file_count > 0 {
        output.push_str("[shared]\n");
        let py_summaries = compute_summaries(py_stats);
        let rs_summaries = compute_summaries(rs_stats);
        
        // Use max of the two for shared metrics
        for py_s in &py_summaries {
            if let Some(key) = shared_config_key(py_s.name) {
                // Find matching Rust summary
                let rs_val = rs_summaries.iter()
                    .find(|s| s.name == py_s.name)
                    .map(|s| s.p99)
                    .unwrap_or(0);
                let max_val = py_s.p99.max(rs_val);
                output.push_str(&format!("{} = {}\n", key, max_val));
            }
        }
    }

    output
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
