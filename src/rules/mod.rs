use kiss::{Config, GateConfig, Language};

mod global;
mod python;
mod rust_rules;
mod test_rules;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThresholdOp {
    AtMost,
    AtLeast,
    StrictLess,
    Equal,
}

impl ThresholdOp {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AtMost => "<=",
            Self::AtLeast => ">=",
            Self::StrictLess => "<",
            Self::Equal => "==",
        }
    }
}

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
    pub(crate) op: ThresholdOp,
    pub(crate) threshold: ThresholdValue,
    pub(crate) description: &'static str,
}

pub fn run_rules(
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
    lang_filter: Option<Language>,
) {
    print_summary_term_definitions();
    print_rule_specs("global", global::GLOBAL_RULE_SPECS, py_config, gate_config);
    print_rule_specs("test", test_rules::TEST_RULE_SPECS, py_config, gate_config);
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
    print_rule_specs(lang, specs, c, g);
}

fn print_rule_specs(section: &str, specs: &[RuleSpec], c: &Config, g: &GateConfig) {
    for spec in specs {
        println!("{}", format_rule_line(section, spec, c, g));
    }
}

fn format_rule_line(section: &str, spec: &RuleSpec, c: &Config, g: &GateConfig) -> String {
    let value = spec.threshold.format(c, g);
    let op = display_op(spec.metric, spec.op, &value);
    let extra = alias_note(section, spec.metric);
    format!(
        "RULE: [{section}] [{} {op} {value}] {}{extra}",
        spec.metric, spec.description
    )
}

fn display_op(metric: &str, op: ThresholdOp, value: &str) -> &'static str {
    if metric == "cycle_size" && value == "0" {
        "=="
    } else {
        op.as_str()
    }
}

fn alias_note(section: &str, metric: &str) -> String {
    let keys = toml_aliases(section, metric);
    if keys.is_empty() {
        return String::new();
    }
    format!(" TOML key(s): {}.", keys.join(", "))
}

fn toml_aliases(section: &str, metric: &str) -> &'static [&'static str] {
    match (section, metric) {
        ("Rust", "positional_args") => &["arguments"],
        (_, "max_indentation_depth") => &["max_indentation"],
        (_, "local_variables_per_function") => &["local_variables"],
        ("Python", "annotations_per_function") => &["decorators_per_function"],
        ("Rust", "annotations_per_function") => &["attributes_per_function"],
        (_, "concrete_types_per_file") => &["classes_per_file", "types_per_file"],
        _ => &[],
    }
}
