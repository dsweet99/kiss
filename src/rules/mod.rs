//! Machine-readable rule output for LLM consumption.
//!
//! This module generates structured RULE: lines with metric IDs, operators, thresholds, and
//! descriptions suitable for parsing by automation/LLMs. For human-friendly sentence templates,
//! see `kiss::rule_defs` in the library crate.
//!
//! Both modules now use canonical metric IDs to ensure consistency.

use kiss::{Config, GateConfig, Language};
use std::path::PathBuf;

mod python;
mod rust_rules;

#[cfg(test)]
mod tests;

pub(crate) enum ThresholdValue {
    Usize(fn(&Config, &GateConfig) -> usize),
    F64(fn(&Config, &GateConfig) -> f64),
}

impl ThresholdValue {
    pub(crate) fn format(&self, c: &Config, g: &GateConfig) -> String {
        match self {
            Self::Usize(f) => f(c, g).to_string(),
            Self::F64(f) => format!("{:.2}", f(c, g)),
        }
    }
}

pub(crate) struct RuleSpec {
    pub(crate) metric: &'static str,
    pub(crate) op: &'static str,
    pub(crate) threshold: ThresholdValue,
    pub(crate) description: &'static str,
}

pub fn run_rules(
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
    lang_filter: Option<Language>,
    _use_defaults: bool,
) {
    print_summary_term_definitions();
    match lang_filter {
        Some(Language::Python) => print_threshold_rules("Python", py_config, gate_config),
        Some(Language::Rust) => print_threshold_rules("Rust", rs_config, gate_config),
        None => {
            print_threshold_rules("Python", py_config, gate_config);
            print_threshold_rules("Rust", rs_config, gate_config);
        }
    }
}

fn print_summary_term_definitions() {
    println!("DEFINITION: [file] A Python or Rust source file included in analysis.");
    println!(
        "DEFINITION: [code_unit] A named unit of code within a file (module, class/type, function, or method) that kiss can attach metrics/violations to."
    );
    println!(
        "DEFINITION: [statement] A statement inside a function/method body (not an import or a class/function signature)."
    );
    println!("DEFINITION: [graph_node] A module (file) in the dependency graph.");
    println!(
        "DEFINITION: [graph_edge] A dependency between two modules (file A depends on file B via imports/uses/mod declarations)."
    );
}

fn print_threshold_rules(lang: &str, c: &Config, g: &GateConfig) {
    let specs = if lang == "Python" {
        python::PY_RULE_SPECS
    } else {
        rust_rules::RS_RULE_SPECS
    };
    for spec in specs {
        println!(
            "RULE: [{lang}] [{} {} {}] {}",
            spec.metric,
            spec.op,
            spec.threshold.format(c, g),
            spec.description
        );
    }
}

pub fn run_config(
    py: &Config,
    rs: &Config,
    gate: &GateConfig,
    config_path: Option<&PathBuf>,
    use_defaults: bool,
) {
    println!("# Effective configuration");
    if use_defaults {
        println!("# Source: built-in defaults");
    } else if let Some(path) = config_path {
        println!("# Source: {}", path.display());
    } else {
        println!("# Source: .kissconfig or ~/.kissconfig (merged)");
    }
    println!("\n[gate]");
    println!("test_coverage_threshold = {}", gate.test_coverage_threshold);
    println!("min_similarity = {:.2}", gate.min_similarity);
    println!("duplication_enabled = {}", gate.duplication_enabled);
    println!("\n[python]");
    print_python_config(py);
    println!("\n[rust]");
    print_rust_config(rs);
}

fn print_python_config(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("statements_per_file = {}", c.statements_per_file);
    println!("lines_per_file = {}", c.lines_per_file);
    println!("positional_args = {}", c.arguments_positional);
    println!("keyword_only_args = {}", c.arguments_keyword_only);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("max_indentation_depth = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!(
        "local_variables_per_function = {}",
        c.local_variables_per_function
    );
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("interface_types_per_file = {}", c.interface_types_per_file);
    println!("concrete_types_per_file = {}", c.concrete_types_per_file);
    println!("imported_names_per_file = {}", c.imported_names_per_file);
    println!("statements_per_try_block = {}", c.statements_per_try_block);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("decorators_per_function = {}", c.annotations_per_function);
    println!("cycle_size = {}", c.cycle_size);
    println!("indirect_dependencies = {}", c.indirect_dependencies);
    println!("dependency_depth = {}", c.dependency_depth);
}

fn print_rust_config(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("statements_per_file = {}", c.statements_per_file);
    println!("lines_per_file = {}", c.lines_per_file);
    println!("arguments_per_function = {}", c.arguments_per_function);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("interface_types_per_file = {}", c.interface_types_per_file);
    println!("concrete_types_per_file = {}", c.concrete_types_per_file);
    println!("max_indentation_depth = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!(
        "local_variables_per_function = {}",
        c.local_variables_per_function
    );
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("imported_names_per_file = {}", c.imported_names_per_file);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("attributes_per_function = {}", c.annotations_per_function);
    println!("cycle_size = {}", c.cycle_size);
    println!("indirect_dependencies = {}", c.indirect_dependencies);
    println!("dependency_depth = {}", c.dependency_depth);
}
