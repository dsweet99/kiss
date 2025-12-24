use clap::{Parser, Subcommand};
use kiss::{
    analyze_file, analyze_graph, analyze_test_refs, build_dependency_graph, cluster_duplicates,
    compute_summaries, detect_duplicates, extract_chunks_for_duplication, find_python_files,
    format_stats_table, generate_config_toml, parse_files, Config, DuplicationConfig, MetricStats,
    ParsedFile,
};
use std::path::{Path, PathBuf};

/// kiss - Python code-quality metrics tool
#[derive(Parser, Debug)]
#[command(name = "kiss", version, about = "Python code-quality metrics tool")]
struct Cli {
    /// Use specified config file instead of defaults
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory to analyze (for default analyze command)
    #[arg(default_value = ".")]
    path: String,
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

    // Load config (from --config flag or default locations)
    let config = if let Some(config_path) = &cli.config {
        Config::load_from(config_path)
    } else {
        Config::load()
    };

    match cli.command {
        Some(Commands::Stats { paths }) => {
            run_stats(&paths, &config);
        }
        Some(Commands::Mimic { paths, out }) => {
            run_mimic(&paths, out.as_deref());
        }
        None => {
            run_analyze(&cli.path, &config);
        }
    }
}

fn run_analyze(path: &str, config: &Config) {
    let root = Path::new(path);
    let files = find_python_files(root);

    if files.is_empty() {
        println!("No Python files found in {}", root.display());
        return;
    }

    let results = match parse_files(&files) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut all_violations = Vec::new();
    let mut parsed_files: Vec<ParsedFile> = Vec::new();

    for result in results {
        match result {
            Ok(parsed) => {
                let violations = analyze_file(&parsed, config);
                all_violations.extend(violations);
                parsed_files.push(parsed);
            }
            Err(e) => {
                eprintln!("Error parsing file: {}", e);
            }
        }
    }

    // Analyze dependency graph
    let parsed_refs: Vec<&ParsedFile> = parsed_files.iter().collect();
    let dep_graph = build_dependency_graph(&parsed_refs);
    let graph_violations = analyze_graph(&dep_graph, config);
    all_violations.extend(graph_violations);

    // Detect duplicates and cluster them
    let dup_config = DuplicationConfig::default();
    let chunks = extract_chunks_for_duplication(&parsed_refs);
    let pairs = detect_duplicates(&parsed_refs, &dup_config);
    let clusters = cluster_duplicates(&pairs, &chunks);

    // Report violations
    if all_violations.is_empty() {
        println!("✓ No violations found in {} files.", files.len());
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

    // Analyze test references
    let test_analysis = analyze_test_refs(&parsed_refs);
    if !test_analysis.unreferenced.is_empty() {
        println!(
            "\n--- Possibly Untested ({} items) ---\n",
            test_analysis.unreferenced.len()
        );
        println!("The following code units are not referenced by any test file.");
        println!("(Note: This is static analysis only; actual coverage may differ.)\n");

        for def in &test_analysis.unreferenced {
            println!("  {}:{} {} '{}'", def.file.display(), def.line, def.kind, def.name);
        }
    }
}

fn run_stats(paths: &[String], _config: &Config) {
    let mut all_stats = MetricStats::default();
    let mut total_files = 0;

    for path in paths {
        let root = Path::new(path);
        let files = find_python_files(root);

        if files.is_empty() {
            eprintln!("Warning: No Python files found in {}", root.display());
            continue;
        }

        let results = match parse_files(&files) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {}", e);
                continue;
            }
        };

        let parsed_files: Vec<ParsedFile> = results.into_iter().filter_map(|r| r.ok()).collect();
        total_files += parsed_files.len();

        let parsed_refs: Vec<&ParsedFile> = parsed_files.iter().collect();
        let mut stats = MetricStats::collect(&parsed_refs);

        // Collect graph metrics (fan-in, fan-out, instability)
        let dep_graph = build_dependency_graph(&parsed_refs);
        stats.collect_graph_metrics(&dep_graph);

        all_stats.merge(stats);
    }

    if total_files == 0 {
        eprintln!("No Python files found in any of the specified paths.");
        std::process::exit(1);
    }

    let summaries = compute_summaries(&all_stats);

    println!("kiss stats - Summary Statistics");
    println!("Analyzed {} files from: {}\n", total_files, paths.join(", "));
    print!("{}", format_stats_table(&summaries));
}

fn run_mimic(paths: &[String], out: Option<&Path>) {
    let mut all_stats = MetricStats::default();
    let mut total_files = 0;

    for path in paths {
        let root = Path::new(path);
        let files = find_python_files(root);

        if files.is_empty() {
            eprintln!("Warning: No Python files found in {}", root.display());
            continue;
        }

        let results = match parse_files(&files) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {}", e);
                continue;
            }
        };

        let parsed_files: Vec<ParsedFile> = results.into_iter().filter_map(|r| r.ok()).collect();
        total_files += parsed_files.len();

        let parsed_refs: Vec<&ParsedFile> = parsed_files.iter().collect();
        let mut stats = MetricStats::collect(&parsed_refs);

        // Collect graph metrics (fan-in, fan-out, instability)
        let dep_graph = build_dependency_graph(&parsed_refs);
        stats.collect_graph_metrics(&dep_graph);

        all_stats.merge(stats);
    }

    if total_files == 0 {
        eprintln!("No Python files found in any of the specified paths.");
        std::process::exit(1);
    }

    let summaries = compute_summaries(&all_stats);
    let config_toml = generate_config_toml(&summaries);

    if let Some(out_path) = out {
        match std::fs::write(out_path, &config_toml) {
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
