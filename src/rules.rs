use kiss::rule_defs::{rules_for_python, rules_for_rust};
use kiss::{Config, GateConfig, Language};
use std::path::PathBuf;

pub fn run_rules(
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
    lang_filter: Option<Language>,
    _use_defaults: bool,
) {
    match lang_filter {
        Some(Language::Python) => print_rules(py_config, gate_config, true),
        Some(Language::Rust) => print_rules(rs_config, gate_config, false),
        None => {
            print_rules(py_config, gate_config, true);
            print_rules(rs_config, gate_config, false);
        }
    }
}

fn print_rules(config: &Config, gate: &GateConfig, is_python: bool) {
    let lang = if is_python { "Python" } else { "Rust" };
    let rules = if is_python { rules_for_python(config, gate) } else { rules_for_rust(config, gate) };
    for (_category, rule_texts) in rules {
        for rule in rule_texts { println!("RULE: [{lang}] {rule}"); }
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
    println!("lines_per_file = {}", c.lines_per_file);
    println!("positional_args = {}", c.arguments_positional);
    println!("keyword_only_args = {}", c.arguments_keyword_only);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("max_indentation = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!("local_variables = {}", c.local_variables_per_function);
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("imports_per_file = {}", c.imports_per_file);
    println!("statements_per_try_block = {}", c.statements_per_try_block);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("decorators_per_function = {}", c.decorators_per_function);
}

fn print_rust_config(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("lines_per_file = {}", c.lines_per_file);
    println!("arguments = {}", c.arguments_per_function);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("types_per_file = {}", c.classes_per_file);
    println!("max_indentation = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!("local_variables = {}", c.local_variables_per_function);
    println!("nested_function_depth = {}", c.nested_function_depth);
    println!("imports_per_file = {}", c.imports_per_file);
    println!("boolean_parameters = {}", c.boolean_parameters);
    println!("attributes_per_function = {}", c.decorators_per_function);
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
        print_rules(&config, &gate, true);
        print_rules(&Config::rust_defaults(), &gate, false);
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
