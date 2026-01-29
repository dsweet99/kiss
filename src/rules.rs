use kiss::{Config, GateConfig, Language};
use std::path::PathBuf;

enum ThresholdValue {
    Usize(fn(&Config, &GateConfig) -> usize),
    F64(fn(&Config, &GateConfig) -> f64),
}

impl ThresholdValue {
    fn format(&self, c: &Config, g: &GateConfig) -> String {
        match self {
            Self::Usize(f) => f(c, g).to_string(),
            Self::F64(f) => format!("{:.2}", f(c, g)),
        }
    }
}

struct RuleSpec {
    metric: &'static str,
    op: &'static str,
    threshold: ThresholdValue,
    description: &'static str,
}

const PY_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec { metric: "statements_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.statements_per_function), description: "statements_per_function is the maximum number of statements in a Python function/method body." },
    RuleSpec { metric: "positional_args", op: "<", threshold: ThresholdValue::Usize(|c, _| c.arguments_positional), description: "positional_args is the maximum number of positional parameters in a Python function definition." },
    RuleSpec { metric: "keyword_only_args", op: "<", threshold: ThresholdValue::Usize(|c, _| c.arguments_keyword_only), description: "keyword_only_args is the maximum number of keyword-only parameters in a Python function definition." },
    RuleSpec { metric: "max_indentation", op: "<", threshold: ThresholdValue::Usize(|c, _| c.max_indentation_depth), description: "max_indentation is the maximum indentation depth within a Python function/method body." },
    RuleSpec { metric: "branches_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.branches_per_function), description: "branches_per_function is the number of if/elif/case_clause branches in a Python function." },
    RuleSpec { metric: "local_variables", op: "<", threshold: ThresholdValue::Usize(|c, _| c.local_variables_per_function), description: "local_variables is the number of distinct local variables assigned in a Python function." },
    RuleSpec { metric: "returns_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.returns_per_function), description: "returns_per_function is the number of return statements in a Python function." },
    RuleSpec { metric: "return_values_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.return_values_per_function), description: "return_values_per_function is the maximum number of values returned by a single return statement in a Python function." },
    RuleSpec { metric: "nested_function_depth", op: "<", threshold: ThresholdValue::Usize(|c, _| c.nested_function_depth), description: "nested_function_depth is the maximum nesting depth of function definitions inside a Python function." },
    RuleSpec { metric: "statements_per_try_block", op: "<", threshold: ThresholdValue::Usize(|c, _| c.statements_per_try_block), description: "statements_per_try_block is the maximum number of statements inside any try block in a Python function." },
    RuleSpec { metric: "boolean_parameters", op: "<", threshold: ThresholdValue::Usize(|c, _| c.boolean_parameters), description: "boolean_parameters is the maximum number of boolean default parameters (True/False) in a Python function." },
    RuleSpec { metric: "decorators_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.annotations_per_function), description: "decorators_per_function is the maximum number of decorators applied to a Python function." },
    RuleSpec { metric: "methods_per_class", op: "<", threshold: ThresholdValue::Usize(|c, _| c.methods_per_class), description: "methods_per_class is the maximum number of methods defined on a Python class." },
    RuleSpec { metric: "statements_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.statements_per_file), description: "statements_per_file is the maximum number of statements inside function/method bodies in a Python file." },
    RuleSpec { metric: "interface_types_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.interface_types_per_file), description: "interface_types_per_file is the maximum number of interface types (Protocol/ABC classes) defined in a Python file." },
    RuleSpec { metric: "concrete_types_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.concrete_types_per_file), description: "concrete_types_per_file is the maximum number of concrete types (non-Protocol/ABC classes) defined in a Python file." },
    RuleSpec { metric: "imported_names_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.imported_names_per_file), description: "imported_names_per_file is the maximum number of unique imported names in a Python file (excluding TYPE_CHECKING-only imports)." },
    RuleSpec { metric: "cycle_size", op: "<", threshold: ThresholdValue::Usize(|c, _| c.cycle_size), description: "cycle_size is the maximum allowed number of modules participating in an import cycle." },
    RuleSpec { metric: "transitive_dependencies", op: "<", threshold: ThresholdValue::Usize(|c, _| c.transitive_dependencies), description: "transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph." },
    RuleSpec { metric: "dependency_depth", op: "<", threshold: ThresholdValue::Usize(|c, _| c.dependency_depth), description: "dependency_depth is the maximum length of an import chain in the dependency graph." },
    RuleSpec { metric: "test_coverage_threshold", op: ">=", threshold: ThresholdValue::Usize(|_, g| g.test_coverage_threshold), description: "test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check)." },
    RuleSpec { metric: "min_similarity", op: ">=", threshold: ThresholdValue::F64(|_, g| g.min_similarity), description: "min_similarity is the minimum similarity required to report duplicate code." },
];

const RS_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec { metric: "statements_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.statements_per_function), description: "statements_per_function is the maximum number of statements in a Rust function/method body." },
    RuleSpec { metric: "arguments", op: "<", threshold: ThresholdValue::Usize(|c, _| c.arguments_per_function), description: "arguments is the maximum number of non-self parameters in a Rust function/method signature." },
    RuleSpec { metric: "max_indentation", op: "<", threshold: ThresholdValue::Usize(|c, _| c.max_indentation_depth), description: "max_indentation is the maximum indentation depth within a Rust function/method body." },
    RuleSpec { metric: "branches_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.branches_per_function), description: "branches_per_function is the number of `if` expressions in a Rust function." },
    RuleSpec { metric: "local_variables", op: "<", threshold: ThresholdValue::Usize(|c, _| c.local_variables_per_function), description: "local_variables is the maximum number of local bindings introduced in a Rust function." },
    RuleSpec { metric: "returns_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.returns_per_function), description: "returns_per_function is the maximum number of `return` expressions in a Rust function." },
    RuleSpec { metric: "nested_function_depth", op: "<", threshold: ThresholdValue::Usize(|c, _| c.nested_function_depth), description: "nested_function_depth is the maximum nesting depth of closures within a Rust function." },
    RuleSpec { metric: "boolean_parameters", op: "<", threshold: ThresholdValue::Usize(|c, _| c.boolean_parameters), description: "boolean_parameters is the maximum number of `bool` parameters in a Rust function signature." },
    RuleSpec { metric: "attributes_per_function", op: "<", threshold: ThresholdValue::Usize(|c, _| c.annotations_per_function), description: "attributes_per_function is the maximum number of non-doc attributes on a Rust function." },
    RuleSpec { metric: "methods_per_class", op: "<", threshold: ThresholdValue::Usize(|c, _| c.methods_per_class), description: "methods_per_class is the maximum number of methods in an `impl` block for a Rust type." },
    RuleSpec { metric: "statements_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.statements_per_file), description: "statements_per_file is the maximum number of statements inside function/method bodies in a Rust file." },
    RuleSpec { metric: "interface_types_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.interface_types_per_file), description: "interface_types_per_file is the maximum number of trait definitions in a Rust file." },
    RuleSpec { metric: "concrete_types_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.concrete_types_per_file), description: "concrete_types_per_file is the maximum number of concrete type definitions (struct/enum/union) in a Rust file." },
    RuleSpec { metric: "imported_names_per_file", op: "<", threshold: ThresholdValue::Usize(|c, _| c.imported_names_per_file), description: "imported_names_per_file is the maximum number of internal `use` statements in a Rust file (excluding `pub use`)." },
    RuleSpec { metric: "cycle_size", op: "<", threshold: ThresholdValue::Usize(|c, _| c.cycle_size), description: "cycle_size is the maximum allowed number of modules participating in a dependency cycle." },
    RuleSpec { metric: "transitive_dependencies", op: "<", threshold: ThresholdValue::Usize(|c, _| c.transitive_dependencies), description: "transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph." },
    RuleSpec { metric: "dependency_depth", op: "<", threshold: ThresholdValue::Usize(|c, _| c.dependency_depth), description: "dependency_depth is the maximum length of a module dependency chain in the dependency graph." },
    RuleSpec { metric: "test_coverage_threshold", op: ">=", threshold: ThresholdValue::Usize(|_, g| g.test_coverage_threshold), description: "test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check)." },
    RuleSpec { metric: "min_similarity", op: ">=", threshold: ThresholdValue::F64(|_, g| g.min_similarity), description: "min_similarity is the minimum similarity required to report duplicate code." },
];

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
    println!("DEFINITION: [code_unit] A named unit of code within a file (module, class/type, function, or method) that kiss can attach metrics/violations to.");
    println!("DEFINITION: [statement] A statement inside a function/method body (not an import or a class/function signature).");
    println!("DEFINITION: [graph_node] A module (file) in the dependency graph.");
    println!("DEFINITION: [graph_edge] A dependency between two modules (file A depends on file B via imports/uses/mod declarations).");
}

fn print_threshold_rules(lang: &str, c: &Config, g: &GateConfig) {
    let specs = if lang == "Python" { PY_RULE_SPECS } else { RS_RULE_SPECS };
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

pub fn run_config(py: &Config, rs: &Config, gate: &GateConfig, config_path: Option<&PathBuf>, use_defaults: bool) {
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
    println!("\n[python]");
    print_python_config(py);
    println!("\n[rust]");
    print_rust_config(rs);
}

fn print_python_config(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("statements_per_file = {}", c.statements_per_file);
    println!("positional_args = {}", c.arguments_positional);
    println!("keyword_only_args = {}", c.arguments_keyword_only);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("max_indentation = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!("local_variables = {}", c.local_variables_per_function);
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("interface_types_per_file = {}", c.interface_types_per_file);
    println!("concrete_types_per_file = {}", c.concrete_types_per_file);
    println!("imported_names_per_file = {}", c.imported_names_per_file);
    println!("statements_per_try_block = {}", c.statements_per_try_block);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("decorators_per_function = {}", c.annotations_per_function);
    println!("cycle_size = {}", c.cycle_size);
    println!("transitive_dependencies = {}", c.transitive_dependencies);
    println!("dependency_depth = {}", c.dependency_depth);
}

fn print_rust_config(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("statements_per_file = {}", c.statements_per_file);
    println!("arguments = {}", c.arguments_per_function);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("interface_types_per_file = {}", c.interface_types_per_file);
    println!("concrete_types_per_file = {}", c.concrete_types_per_file);
    println!("max_indentation = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!("local_variables = {}", c.local_variables_per_function);
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("imported_names_per_file = {}", c.imported_names_per_file);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("attributes_per_function = {}", c.annotations_per_function);
    println!("cycle_size = {}", c.cycle_size);
    println!("transitive_dependencies = {}", c.transitive_dependencies);
    println!("dependency_depth = {}", c.dependency_depth);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rules_functions_no_panic() {
        let py_config = Config::python_defaults();
        let rs_config = Config::rust_defaults();
        let gate_config = GateConfig::default();

        run_rules(&py_config, &rs_config, &gate_config, None, false);
        run_rules(&py_config, &rs_config, &gate_config, Some(Language::Python), true);
        run_rules(&py_config, &rs_config, &gate_config, Some(Language::Rust), true);
    }

    #[test]
    fn test_print_rules() {
        let config = Config::python_defaults();
        let gate = GateConfig::default();
        print_summary_term_definitions();
        print_threshold_rules("Python", &config, &gate);
        print_threshold_rules("Rust", &Config::rust_defaults(), &gate);
    }
    #[test]
    fn test_run_config_and_print_config() {
        let py = Config::python_defaults();
        let rs = Config::rust_defaults();
        let gate = GateConfig::default();
        run_config(&py, &rs, &gate, None, false);
        run_config(&py, &rs, &gate, None, true);
        print_python_config(&py);
        print_rust_config(&rs);
    }
}
