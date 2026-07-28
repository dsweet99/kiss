use clap::{Parser, Subcommand};
use kiss::Language;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "kiss",
    version,
    about = "Code-quality metrics tool for Python and Rust"
)]
#[command(
    after_help = "EXAMPLES:\n  kiss check .                 Run static analysis on current directory\n  kiss check . src/module/     Analyze module against full codebase (focus mode)\n  kiss cov .                   Refresh and check runtime line coverage\n  kiss check --lang rust src/  Analyze only Rust files in src/\n  kiss mimic . --out .kissconfig   Generate config from codebase\n  kiss init .                  Write a default .kissconfig"
)]
pub struct Cli {
    /// Path to custom config file (default: .kissconfig)
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

pub fn parse_positive_usize(s: &str) -> Result<usize, String> {
    let value = s
        .parse::<usize>()
        .map_err(|_| format!("expected a positive integer, got '{s}'"))?;
    if value == 0 {
        return Err("expected a positive integer, got '0'".to_string());
    }
    Ok(value)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TestInvocation {
    Commit,
    Base,
    Main,
    All,
    Targets(Vec<String>),
}

const RESERVED_TEST_ACTIONS: &[&str] = &["commit", "base", "main", "all"];

pub fn parse_test_invocation(operands: &[String]) -> Result<TestInvocation, String> {
    let first = operands.first().ok_or_else(|| {
        "at least one of commit, base, main, all, or a PATH / PATH::symbol target is required"
            .to_string()
    })?;
    if let Some(reserved) = parse_reserved_action(first, operands.len())? {
        return Ok(reserved);
    }
    if matches!(first.as_str(), "cov" | "validate-selection") {
        return Err(format!(
            "unknown test target '{first}'. Use commit, base, main, all, or PATH / PATH::symbol. \
             Coverage is `kiss cov`."
        ));
    }
    if let Some(operand) = operands
        .iter()
        .find(|operand| RESERVED_TEST_ACTIONS.contains(&operand.as_str()))
    {
        return Err(format!(
            "reserved action '{operand}' cannot be mixed with PATH / PATH::symbol targets"
        ));
    }
    for operand in operands {
        validate_target_operand_shape(operand)?;
    }
    Ok(TestInvocation::Targets(operands.to_vec()))
}

fn validate_target_operand_shape(raw: &str) -> Result<(), String> {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    if path_part.is_empty() {
        return Err(format!("unknown test target '{raw}'. Use commit, base, main, all, or PATH / PATH::symbol."));
    }
    let ok_ext = std::path::Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"));
    if !ok_ext {
        return Err(format!(
            "unknown test target '{raw}'. Use commit, base, main, all, or PATH / PATH::symbol."
        ));
    }
    if let Some((_, symbol)) = raw.split_once("::")
        && symbol.is_empty()
    {
        return Err("target path and symbol must both be non-empty".to_string());
    }
    Ok(())
}

fn parse_reserved_action(first: &str, operand_count: usize) -> Result<Option<TestInvocation>, String> {
    if !RESERVED_TEST_ACTIONS.contains(&first) {
        return Ok(None);
    }
    if operand_count > 1 {
        return Err(format!(
            "reserved action '{first}' cannot be mixed with additional targets"
        ));
    }
    Ok(Some(match first {
        "commit" => TestInvocation::Commit,
        "base" => TestInvocation::Base,
        "main" => TestInvocation::Main,
        "all" => TestInvocation::All,
        _ => unreachable!("reserved action list must match match arms"),
    }))
}

pub fn validate_test_branch_options(
    invocation: &TestInvocation,
    main_branch: Option<&str>,
    base_branch: Option<&str>,
) -> Result<(), String> {
    match invocation {
        TestInvocation::Main => {
            if base_branch.is_some() {
                return Err("--base-branch is only valid with kiss test base".to_string());
            }
        }
        TestInvocation::Base => {
            if main_branch.is_some() {
                return Err("--main-branch is only valid with kiss test main".to_string());
            }
        }
        TestInvocation::Commit | TestInvocation::All | TestInvocation::Targets(_) => {
            if main_branch.is_some() {
                return Err("--main-branch is only valid with kiss test main".to_string());
            }
            if base_branch.is_some() {
                return Err("--base-branch is only valid with kiss test base".to_string());
            }
        }
    }
    Ok(())
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run static complexity, graph, and duplicate checks
    Check {
        /// First path is UNIVERSE (analysis scope), additional paths are FOCUS (report only these)
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        /// Show timing breakdown for performance analysis
        #[arg(long)]
        timing: bool,
    },
    /// Refresh and check runtime line coverage
    Cov {
        /// First path is UNIVERSE (coverage scope), additional paths are FOCUS (report only these)
        #[arg(default_value = ".")]
        paths: Vec<String>,
        /// Bypass coverage gate and show all coverage violations
        #[arg(long)]
        all: bool,
        /// Ignore files/directories starting with PREFIX (repeatable)
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        /// Show timing breakdown for coverage loading/refresh/evaluation
        #[arg(long)]
        timing: bool,
        /// Maximum test jobs when refreshing runtime coverage
        #[arg(short = 'j', long, value_name = "JOBS", value_parser = parse_positive_usize)]
        jobs: Option<usize>,
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
        #[arg(long, default_value = "0.9")]
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
        /// Coarsen the graph to approximately N nodes (mutually exclusive with --zoom).
        #[arg(long, value_name = "N", conflicts_with = "zoom")]
        num_nodes: Option<usize>,
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
    /// Run pytest / cargo nextest for covering tests (git modes, all, or PATH targets)
    #[command(alias = "t")]
    Test {
        /// `commit`, `base`, `main`, `all`, or one or more `PATH` / `PATH::symbol` targets
        #[arg(
            required = true,
            num_args = 1..,
            value_name = "commit|base|main|all|TARGET"
        )]
        operands: Vec<String>,
        #[arg(long, value_name = "BRANCH")]
        main_branch: Option<String>,
        #[arg(long, value_name = "BRANCH")]
        base_branch: Option<String>,
        #[arg(long)]
        dry_run: bool,
        /// Force selected tests to rerun instead of reusing test-runner caches
        #[arg(long)]
        force: bool,
        /// Print a local rubric metrics summary for this run
        #[arg(long)]
        metrics: bool,
        /// Maximum number of test jobs to run concurrently
        #[arg(short = 'j', long, default_value_t = 1)]
        jobs: usize,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        #[arg(last = true)]
        extra: Vec<String>,
    },
    /// Internal target-runner shim for compile-once Rust coverage.
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

#[cfg(test)]
#[path = "args_test.rs"]
mod coverage_witness;
