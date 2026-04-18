use clap::{Parser, Subcommand};
use kiss::Language;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "kiss",
    version,
    about = "Code-quality metrics tool for Python and Rust"
)]
#[command(
    after_help = "EXAMPLES:\n  kiss check .                 Analyze current directory\n  kiss check . src/module/     Analyze module against full codebase (focus mode)\n  kiss check --lang rust src/  Analyze only Rust files in src/\n  kiss mimic . --out .kissconfig   Generate config from codebase\n  kiss init .                  Write a default .kissconfig"
)]
pub struct Cli {
    /// Path to custom config file (default: .kissconfig or ~/.kissconfig)
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Filter by language: python (py) or rust (rs)
    #[arg(long, global = true, value_parser = parse_language, value_name = "LANG")]
    pub lang: Option<Language>,

    /// Use built-in defaults, ignoring config files
    #[arg(long, global = true)]
    pub defaults: bool,

    #[command(subcommand)]
    pub command: Commands,
}

pub fn parse_language(s: &str) -> Result<Language, String> {
    match s.to_lowercase().as_str() {
        "python" | "py" => Ok(Language::Python),
        "rust" | "rs" => Ok(Language::Rust),
        _ => Err(format!("Unknown language '{s}'. Use 'python' or 'rust'.")),
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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
    Clamp {
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Write a default .kissconfig into `REPO_PATH` (defaults to current directory)
    Init {
        /// Repository path where `.kissconfig` should be written
        #[arg(default_value = ".")]
        repo_path: PathBuf,
    },
    /// Detect duplicate code blocks (uses function-level chunks)
    Dry {
        /// Path to scan for duplicates
        #[arg(default_value = ".")]
        path: String,
        /// Optional file paths to filter results (only report duplicates involving these files)
        #[arg(value_name = "FILTER_FILES")]
        filter_files: Vec<String>,
        /// Character n-gram size for shingling (default matches `kiss check`)
        #[arg(long, default_value = "3")]
        shingle_size: usize,
        /// Number of `MinHash` functions (default matches `kiss check`)
        #[arg(long, default_value = "100")]
        minhash_size: usize,
        /// Number of LSH bands (default matches `kiss check`)
        #[arg(long, default_value = "20")]
        lsh_bands: usize,
        /// Minimum similarity threshold [0.0-1.0] (default matches `kiss check`)
        #[arg(long, default_value = "0.7")]
        min_similarity: f64,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Display all available rules and their current thresholds
    Rules,
    /// Show effective configuration (merged from all sources)
    Config,
    /// Write dependency graph (Mermaid or Graphviz DOT based on output extension)
    Viz {
        /// Output file path. Format is inferred from extension:
        /// - `.md`: Markdown with a Mermaid code fence
        /// - `.mmd` / `.mermaid`: Mermaid diagram text
        /// - `.dot`: Graphviz DOT
        out: PathBuf,
        /// Paths to analyze
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Coarsen the graph [0,1]. 0 collapses to one node; 1 shows all nodes (default: 1).
        #[arg(long, value_name = "Z", default_value = "1.0")]
        zoom: f64,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Constrained minimization: `kiss shrink METRIC=VALUE` to start, `kiss shrink` to check
    Shrink {
        /// Omit to check against saved constraints.
        #[arg(
            value_name = "METRIC=VALUE",
            help = "Target metric and value (metrics: files, code_units, statements, graph_nodes, graph_edges)"
        )]
        target: Option<String>,
        /// Paths to analyze
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Show which tests kiss detects for specified source files
    #[command(alias = "st")]
    ShowTests {
        /// Source file paths to inspect
        #[arg(required = true)]
        paths: Vec<String>,
        /// Also show untested definitions
        #[arg(long)]
        untested: bool,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    /// Semantic rename/move for Python and Rust symbols (beta)
    Mv {
        /// Source symbol (`path.py::name`, `path.py::Class.method`, `path.rs::name`, `path.rs::Type.method`)
        #[arg(value_name = "SOURCE")]
        query: String,
        /// Target name (bare identifier for the renamed symbol)
        #[arg(value_name = "TARGET")]
        new_name: String,
        /// Paths to analyze for references
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Destination file path for symbol moves
        #[arg(long, value_name = "DEST_FILE")]
        to: Option<PathBuf>,
        /// Print planned edits without applying writes
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-stable JSON output
        #[arg(long)]
        json: bool,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
}
