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

pub fn run_config(py: &Config, rs: &Config, gate: &GateConfig, config_path: Option<&PathBuf>) {
    println!("# Effective configuration");
    if let Some(path) = config_path {
        println!("# Source: {}", path.display());
    } else {
        println!("# Source: .kissconfig or ~/.kissconfig (merged)");
    }
    println!("\n[gate]");
    println!("test_coverage_threshold = {}", gate.test_coverage_threshold);
    println!("min_similarity = {:.2}", gate.min_similarity);
    println!("\n[python]");
    print_config_values(py);
    println!("\n[rust]");
    print_config_values(rs);
}

fn print_config_values(c: &Config) {
    println!("statements_per_function = {}", c.statements_per_function);
    println!("lines_per_file = {}", c.lines_per_file);
    println!("arguments_per_function = {}", c.arguments_per_function);
    println!("methods_per_class = {}", c.methods_per_class);
    println!("max_indentation_depth = {}", c.max_indentation_depth);
    println!("branches_per_function = {}", c.branches_per_function);
    println!("returns_per_function = {}", c.returns_per_function);
    println!("local_variables_per_function = {}", c.local_variables_per_function);
    println!("imports_per_file = {}", c.imports_per_file);
    println!("classes_per_file = {}", c.classes_per_file);
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
}
