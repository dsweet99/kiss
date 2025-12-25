//! Coding rules output for LLM context priming

use std::path::Path;

use kiss::{Config, GateConfig, Language};

pub fn run_rules(
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
    lang_filter: Option<Language>,
    use_defaults: bool,
) {
    let config_source = if use_defaults {
        "built-in defaults"
    } else if Path::new(".kissconfig").exists() {
        ".kissconfig"
    } else if std::env::var_os("HOME").is_some_and(|h| Path::new(&h).join(".kissconfig").exists()) {
        "~/.kissconfig"
    } else {
        "built-in defaults"
    };

    println!("# kiss coding rules\n");
    println!("*Source: {config_source}*\n");

    match lang_filter {
        Some(Language::Python) => print_python_rules(py_config, gate_config),
        Some(Language::Rust) => print_rust_rules(rs_config, gate_config),
        None => {
            print_python_rules(py_config, gate_config);
            println!();
            print_rust_rules(rs_config, gate_config);
        }
    }
}

pub fn print_python_rules(config: &Config, gate: &GateConfig) {
    println!("## Python\n");
    println!("### Functions\n");
    println!(
        "- Keep functions ≤ {} statements",
        config.statements_per_function
    );
    println!(
        "- Use ≤ {} positional arguments; prefer keyword-only args after that",
        config.arguments_positional
    );
    println!(
        "- Limit keyword-only arguments to ≤ {}",
        config.arguments_keyword_only
    );
    println!(
        "- Keep indentation depth ≤ {} levels",
        config.max_indentation_depth
    );
    println!(
        "- Limit branches (if/elif/else) to ≤ {} per function",
        config.branches_per_function
    );
    println!(
        "- Keep local variables ≤ {} per function",
        config.local_variables_per_function
    );
    println!(
        "- Limit return statements to ≤ {} per function",
        config.returns_per_function
    );
    println!(
        "- Avoid deeply nested functions (max depth: {})",
        config.nested_function_depth
    );
    println!();
    println!("### Classes\n");
    println!("- Keep methods per class ≤ {}", config.methods_per_class);
    println!(
        "- Ensure methods share instance fields (LCOM ≤ {}%)",
        config.lcom
    );
    println!();
    println!("### Files\n");
    println!("- Keep files ≤ {} lines", config.lines_per_file);
    println!("- Limit to ≤ {} classes per file", config.classes_per_file);
    println!("- Keep imports ≤ {} per file", config.imports_per_file);
    println!();
    print_shared_rules(config, gate);
}

pub fn print_rust_rules(config: &Config, gate: &GateConfig) {
    println!("## Rust\n");
    println!("### Functions\n");
    println!(
        "- Keep functions ≤ {} statements",
        config.statements_per_function
    );
    println!("- Limit arguments to ≤ {}", config.arguments_per_function);
    println!(
        "- Keep indentation depth ≤ {} levels",
        config.max_indentation_depth
    );
    println!(
        "- Limit branches (if/match/loop) to ≤ {} per function",
        config.branches_per_function
    );
    println!(
        "- Keep local variables ≤ {} per function",
        config.local_variables_per_function
    );
    println!(
        "- Limit return statements to ≤ {} per function",
        config.returns_per_function
    );
    println!(
        "- Avoid deeply nested closures (max depth: {})",
        config.nested_function_depth
    );
    println!();
    println!("### Types\n");
    println!("- Keep methods per type ≤ {}", config.methods_per_class);
    println!("- Ensure methods share fields (LCOM ≤ {}%)", config.lcom);
    println!();
    println!("### Files\n");
    println!("- Keep files ≤ {} lines", config.lines_per_file);
    println!("- Limit to ≤ {} types per file", config.classes_per_file);
    println!(
        "- Keep imports (use statements) ≤ {} per file",
        config.imports_per_file
    );
    println!();
    print_shared_rules(config, gate);
}

fn print_shared_rules(config: &Config, gate: &GateConfig) {
    println!("### Dependencies\n");
    println!("- Avoid circular dependencies");
    println!(
        "- Limit fan-out (direct dependencies) to ≤ {}",
        config.fan_out
    );
    println!(
        "- Keep fan-in modules stable and well-tested (threshold: {})",
        config.fan_in
    );
    println!();
    println!("### Testing\n");
    println!("- Every function/class/type should be referenced by tests");
    println!(
        "- Maintain ≥ {}% test reference coverage",
        gate.test_coverage_threshold
    );
    println!();
    println!("### Duplication\n");
    println!("- Avoid copy-pasted code blocks");
    println!("- Factor out repeated patterns into shared functions");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rules_functions_no_panic() {
        let py_config = Config::python_defaults();
        let rs_config = Config::rust_defaults();
        let gate_config = GateConfig::default();

        print_python_rules(&py_config, &gate_config);
        print_rust_rules(&rs_config, &gate_config);
        run_rules(&py_config, &rs_config, &gate_config, None, false);
        run_rules(&py_config, &rs_config, &gate_config, Some(Language::Python), true);
    }
}

