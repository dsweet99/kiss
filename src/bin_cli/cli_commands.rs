use clap::Subcommand;
use std::ffi::OsString;
use std::path::PathBuf;

use super::parse_positive_usize;

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Run static complexity, graph, duplicate, comment, doc, and orphan checks")]
    Check {
        #[arg(
            default_value = ".",
            value_name = "PATH",
            help = "Codebase root, optionally followed by focus paths"
        )]
        paths: Vec<String>,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
        #[arg(long, help = "Print analysis stage timings")]
        timing: bool,
    },
    #[command(about = "Show metric statistics for the codebase")]
    Stats {
        #[arg(
            default_value = ".",
            value_name = "PATH",
            help = "Files or directories to analyze"
        )]
        paths: Vec<String>,
        #[arg(
            long,
            value_name = "N",
            default_missing_value = "10",
            num_args = 0..=1,
            require_equals = true,
            help = "Print top-N units per metric (default 10)"
        )]
        all: Option<usize>,
        #[arg(long, help = "Print a table of all metric values")]
        table: bool,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
    #[command(about = "Generate .kissconfig thresholds from an existing codebase")]
    Mimic {
        #[arg(
            required = true,
            value_name = "PATH",
            help = "Files or directories to analyze"
        )]
        paths: Vec<String>,
        #[arg(
            long,
            short,
            value_name = "FILE",
            help = "Write config to FILE instead of stdout"
        )]
        out: Option<PathBuf>,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
    #[command(
        about = "Generate .kissconfig from the current directory",
        after_help = "Same as: kiss mimic . --out .kissconfig"
    )]
    Clamp {
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
    #[command(about = "Write a default .kissconfig")]
    Init {
        #[arg(default_value = ".", help = "Directory to write .kissconfig into")]
        repo_path: PathBuf,
    },
    #[command(about = "Detect duplicate code blocks")]
    Dry {
        #[arg(default_value = ".", help = "Root path to scan")]
        path: String,
        #[arg(value_name = "FILTER_FILES", help = "Limit comparison to these files")]
        filter_files: Vec<String>,
        #[arg(
            long,
            default_value = "3",
            help = "Shingle size for similarity hashing"
        )]
        shingle_size: usize,
        #[arg(long, default_value = "100", help = "MinHash permutation count")]
        minhash_size: usize,
        #[arg(long, default_value = "20", help = "Number of LSH bands")]
        lsh_bands: usize,
        #[arg(
            long,
            help = "Minimum similarity to report (default: .kissconfig min_similarity)"
        )]
        min_similarity: Option<f64>,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
    #[command(about = "Display all available rules and their current thresholds")]
    Rules,
    #[command(about = "Write a dependency graph")]
    Viz {
        #[arg(help = "Output file (.md, .mmd/.mermaid, or .dot)")]
        out: PathBuf,
        #[arg(
            default_value = ".",
            value_name = "PATH",
            help = "Files or directories to analyze"
        )]
        paths: Vec<String>,
        #[arg(
            long,
            value_name = "Z",
            default_value = "1.0",
            help = "Graph coarsening factor (0.0-1.0)"
        )]
        zoom: f64,
        #[arg(
            long,
            value_name = "N",
            conflicts_with = "zoom",
            help = "Target number of graph nodes"
        )]
        num_nodes: Option<usize>,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
    #[command(
        alias = "t",
        about = "Run covering tests and enforce coverage and time gates"
    )]
    Test {
        #[arg(
            num_args = 0..,
            value_name = "TARGET",
            default_value = ".",
            help = "commit, base, main, ., or PATH / PATH::symbol / directory"
        )]
        operands: Vec<String>,
        #[arg(long, value_name = "BRANCH", help = "Branch name for kiss test main")]
        main_branch: Option<String>,
        #[arg(long, value_name = "BRANCH", help = "Branch name for kiss test base")]
        base_branch: Option<String>,
        #[arg(long, help = "Show tests that would run without executing them")]
        dry_run: bool,
        #[arg(
            long,
            help = "Force selected tests to rerun instead of reusing test-runner caches"
        )]
        force: bool,
        #[arg(
            long,
            help = "Rerun tests that need it under normal rules, plus any marked FAIL or TIMEOUT"
        )]
        force_bad: bool,
        #[arg(long, help = "Print test-run metrics")]
        metrics: bool,
        #[arg(long, help = "Include files that currently pass the coverage gate")]
        coverage_all: bool,
        #[arg(long, help = "Rerun tests when sources change")]
        watch: bool,
        #[arg(
            short = 'j',
            long,
            value_name = "JOBS",
            value_parser = parse_positive_usize,
            help = "Maximum number of test jobs to run concurrently"
        )]
        jobs: Option<usize>,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
        #[arg(
            last = true,
            value_name = "ARG",
            help = "Arguments passed through to the test runner"
        )]
        extra: Vec<String>,
    },
    #[command(
        name = "__coverage",
        hide = true,
        about = "Coverage-only evaluation (prefer kiss test for the full path)"
    )]
    Coverage {
        #[arg(
            default_value = ".",
            value_name = "PATH",
            help = "Files or directories to analyze"
        )]
        paths: Vec<String>,
        #[arg(long, help = "Include files that currently pass the coverage gate")]
        all: bool,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
        #[arg(long, help = "Print coverage stage timings")]
        timing: bool,
        #[arg(
            short = 'j',
            long,
            value_name = "JOBS",
            value_parser = parse_positive_usize,
            help = "Maximum number of coverage jobs to run concurrently"
        )]
        jobs: Option<usize>,
    },
    #[command(name = "__rust-llvm-cov-target-runner", hide = true)]
    RustLlvmCovTargetRunner {
        #[arg(long, value_name = "DIR")]
        output_dir: PathBuf,
        #[arg(long, value_name = "PATH")]
        runner_map: PathBuf,
        #[arg(long, value_name = "TRIPLE")]
        platform: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    #[command(about = "Rename or move a Python or Rust symbol (beta)")]
    Mv {
        #[arg(value_name = "SOURCE", help = "Symbol to rename (PATH::symbol)")]
        query: String,
        #[arg(value_name = "TARGET", help = "New symbol name")]
        new_name: String,
        #[arg(
            default_value = ".",
            value_name = "PATH",
            help = "Files or directories to search"
        )]
        paths: Vec<String>,
        #[arg(
            long,
            value_name = "DEST_FILE",
            help = "Write the renamed symbol to DEST_FILE"
        )]
        to: Option<PathBuf>,
        #[arg(long, help = "Show the rename without writing files")]
        dry_run: bool,
        #[arg(long, help = "Print JSON instead of text")]
        json: bool,
        #[arg(long, value_name = "PREFIX", help = "Path prefix to exclude")]
        ignore: Vec<String>,
    },
}
