use kiss::rule_defs::{rules_for_python, rules_for_rust};
use kiss::{Config, GateConfig, Language};

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
    let rules = if is_python {
        rules_for_python(config, gate)
    } else {
        rules_for_rust(config, gate)
    };

    for (_category, rule_texts) in rules {
        for rule in rule_texts {
            println!("RULE: [{lang}] {rule}");
        }
    }
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
}
