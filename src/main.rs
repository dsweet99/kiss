use clap::Parser;
use kiss::{
    analyze_file, analyze_graph, build_dependency_graph, cluster_duplicates, detect_duplicates,
    extract_chunks_for_duplication, find_python_files, parse_files, Config, DuplicationConfig,
    ParsedFile,
};
use std::path::Path;

/// kiss - Python code-quality metrics tool
#[derive(Parser, Debug)]
#[command(name = "kiss", version, about = "Python code-quality metrics tool")]
struct Args {
    /// Directory to analyze (defaults to current directory)
    #[arg(default_value = ".")]
    path: String,
}

fn main() {
    let args = Args::parse();
    let root = Path::new(&args.path);

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

    let config = Config::load();
    let mut all_violations = Vec::new();
    let mut parsed_files: Vec<ParsedFile> = Vec::new();

    for result in results {
        match result {
            Ok(parsed) => {
                let violations = analyze_file(&parsed, &config);
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
    let graph_violations = analyze_graph(&dep_graph, &config);
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
}

