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
    after_help = "EXAMPLES:\n  kiss check .                 Run static analysis on current directory\n  kiss check . src/module/     Analyze module against full codebase (focus mode)\n  kiss test                    Run tests and enforce runtime coverage\n  kiss check --lang rust src/  Analyze only Rust files in src/\n  kiss mimic . --out .kissconfig   Generate config from codebase\n  kiss init .                  Write a default .kissconfig"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, value_parser = parse_language, value_name = "LANG")]
    pub lang: Option<Language>,

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

const RESERVED_TEST_ACTIONS: &[&str] = &["commit", "base", "main"];
const TEST_OPERAND_HINT: &str =
    "commit, base, main, ., or PATH / PATH::symbol / directory";

fn is_dot_all_operand(operand: &str) -> bool {
    matches!(operand, "." | "./")
}

fn path_has_source_ext(path_part: &str) -> bool {
    std::path::Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

pub fn parse_test_invocation(operands: &[String]) -> Result<TestInvocation, String> {

    if operands.is_empty() {
        return Ok(TestInvocation::All);
    }
    let first = &operands[0];
    reject_legacy_all_operand(operands)?;
    if let Some(reserved) = parse_reserved_action(first, operands.len())? {
        return Ok(reserved);
    }
    if let Some(invocation) = try_parse_dot_all(operands, first)? {
        return Ok(invocation);
    }
    parse_path_or_directory_targets(operands, first)
}

fn reject_legacy_all_operand(operands: &[String]) -> Result<(), String> {
    if operands.iter().any(|operand| operand == "all") {
        Err(
            "unknown test target 'all'. Use `kiss test .` instead of `kiss test all`.".to_string(),
        )
    } else {
        Ok(())
    }
}

fn try_parse_dot_all(
    operands: &[String],
    first: &str,
) -> Result<Option<TestInvocation>, String> {
    if is_dot_all_operand(first) {
        if operands.len() > 1 {
            return Err("`.` cannot be mixed with additional targets".to_string());
        }
        return Ok(Some(TestInvocation::All));
    }
    if operands.iter().any(|operand| is_dot_all_operand(operand)) {
        return Err("`.` cannot be mixed with additional targets".to_string());
    }
    Ok(None)
}

fn parse_path_or_directory_targets(
    operands: &[String],
    first: &str,
) -> Result<TestInvocation, String> {
    if matches!(first, "cov" | "validate-selection") {
        return Err(format!(
            "unknown test target '{first}'. Use {TEST_OPERAND_HINT}. Coverage is enforced by `kiss test`."
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
    let (path_part, symbol) = match raw.split_once("::") {
        Some((path, symbol)) => (path, Some(symbol)),
        None => (raw, None),
    };
    if path_part.is_empty() {
        return Err(format!(
            "unknown test target '{raw}'. Use {TEST_OPERAND_HINT}."
        ));
    }
    if let Some(symbol) = symbol {
        if !path_has_source_ext(path_part) {
            return Err(format!(
                "unknown test target '{raw}'. PATH::symbol requires a .py or .rs path."
            ));
        }
        if symbol.is_empty() {
            return Err("target path and symbol must both be non-empty".to_string());
        }
        return Ok(());
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
    Check {
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        #[arg(long)]
        timing: bool,
    },
    Stats {
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long, value_name = "N", default_missing_value = "10", num_args = 0..=1, require_equals = true)]
        all: Option<usize>,
        #[arg(long)]
        table: bool,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    Mimic {
        #[arg(required = true)]
        paths: Vec<String>,
        #[arg(long, short, value_name = "FILE")]
        out: Option<PathBuf>,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    Clamp {
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    Init {
        #[arg(default_value = ".")]
        repo_path: PathBuf,
    },
    Dry {
        #[arg(default_value = ".")]
        path: String,
        #[arg(value_name = "FILTER_FILES")]
        filter_files: Vec<String>,
        #[arg(long, default_value = "3")]
        shingle_size: usize,
        #[arg(long, default_value = "100")]
        minhash_size: usize,
        #[arg(long, default_value = "20")]
        lsh_bands: usize,
        #[arg(long, default_value = "0.9")]
        min_similarity: f64,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    Rules,
    Config,
    Viz {
        out: PathBuf,
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long, value_name = "Z", default_value = "1.0")]
        zoom: f64,
        #[arg(long, value_name = "N", conflicts_with = "zoom")]
        num_nodes: Option<usize>,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    Shrink {
        #[arg(
            value_name = "METRIC=VALUE",
            help = "Target metric and value (metrics: files, code_units, statements, graph_nodes, graph_edges)"
        )]
        target: Option<String>,
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
    #[command(alias = "t")]
    Test {
        #[arg(
            num_args = 0..,
            value_name = "commit|base|main|.|TARGET",
            default_value = "."
        )]
        operands: Vec<String>,
        #[arg(long, value_name = "BRANCH")]
        main_branch: Option<String>,
        #[arg(long, value_name = "BRANCH")]
        base_branch: Option<String>,
        #[arg(long)]
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
        #[arg(long)]
        metrics: bool,
        #[arg(long)]
        coverage_all: bool,
        #[arg(long)]
        watch: bool,
        #[arg(
            short = 'j',
            long,
            value_name = "JOBS",
            value_parser = parse_positive_usize,
            help = "Maximum number of test jobs to run concurrently"
        )]
        jobs: Option<usize>,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        #[arg(last = true)]
        extra: Vec<String>,
    },
    #[command(name = "cov", alias = "__coverage")]
    Coverage {
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
        #[arg(long)]
        timing: bool,
        #[arg(short = 'j', long, value_name = "JOBS", value_parser = parse_positive_usize)]
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
    Mv {
        #[arg(value_name = "SOURCE")]
        query: String,
        #[arg(value_name = "TARGET")]
        new_name: String,
        #[arg(default_value = ".")]
        paths: Vec<String>,
        #[arg(long, value_name = "DEST_FILE")]
        to: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, value_name = "PREFIX")]
        ignore: Vec<String>,
    },
}

#[cfg(test)]
#[path = "args_test.rs"]
mod coverage_witness;
