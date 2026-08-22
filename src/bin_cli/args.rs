use clap::Parser;
use kiss::Language;
use std::path::PathBuf;

#[path = "cli_commands.rs"]
mod cli_commands;
pub use cli_commands::Commands;

const AFTER_HELP: &str = "\
Examples:
  kiss check .                    Run static analysis; write .kissconfig if missing
  kiss check . src/module/        Check one module against the full codebase
  kiss test                       Run tests and enforce runtime coverage
  kiss check --lang rust src/     Analyze only Rust files in src/
  kiss viz graph.md               Write a Mermaid dependency graph
";

#[derive(Parser, Debug)]
#[command(
    name = "kiss",
    version,
    about = "Global code feedback for LLM coding agents",
    disable_help_subcommand = true
)]
#[command(after_help = AFTER_HELP)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Path to custom config file (default: .kissconfig)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_parser = parse_language,
        value_name = "LANG",
        help = "Filter by language: python (py) or rust (rs)"
    )]
    pub lang: Option<Language>,

    #[arg(
        long,
        global = true,
        help = "Use built-in defaults, ignoring config files"
    )]
    pub defaults: bool,

    #[command(subcommand)]
    pub command: Commands,
}

pub fn parse_language(s: &str) -> Result<Language, String> {
    match s.to_lowercase().as_str() {
        "python" | "py" => Ok(Language::Python),
        "rust" | "rs" => Ok(Language::Rust),
        _ => Err(format!(
            "unknown language '{s}'; use python, py, rust, or rs"
        )),
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
const TEST_OPERAND_HINT: &str = "commit, base, main, ., or PATH / PATH::symbol / directory";

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
        Err("unknown test target 'all'. Use `kiss test .` instead of `kiss test all`.".to_string())
    } else {
        Ok(())
    }
}

fn try_parse_dot_all(operands: &[String], first: &str) -> Result<Option<TestInvocation>, String> {
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

fn parse_reserved_action(
    first: &str,
    operand_count: usize,
) -> Result<Option<TestInvocation>, String> {
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

#[cfg(test)]
#[path = "args_test.rs"]
mod coverage_witness;
