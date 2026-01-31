use crate::config::Config;
use crate::gate_config::GateConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Functions,
    Classes,
    Files,
    Dependencies,
    Testing,
    Duplication,
}

impl RuleCategory {
    pub const fn python_heading(self) -> &'static str {
        match self {
            Self::Functions => "Functions",
            Self::Classes => "Classes",
            Self::Files => "Files",
            Self::Dependencies => "Dependencies",
            Self::Testing => "Testing",
            Self::Duplication => "Duplication",
        }
    }

    pub const fn rust_heading(self) -> &'static str {
        match self {
            Self::Functions => "Functions",
            Self::Classes => "Types",
            Self::Files => "Files",
            Self::Dependencies => "Dependencies",
            Self::Testing => "Testing",
            Self::Duplication => "Duplication",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Applicability {
    Python,
    Rust,
    Both,
}

pub struct Rule {
    pub category: RuleCategory,
    pub template: &'static str,
    pub get_threshold: fn(&Config, &GateConfig) -> usize,
    pub applicability: Applicability,
}

impl Rule {
    pub fn format(&self, config: &Config, gate: &GateConfig) -> String {
        self.template.replace("{}", &(self.get_threshold)(config, gate).to_string())
    }

    pub const fn applies_to_python(&self) -> bool {
        matches!(self.applicability, Applicability::Python | Applicability::Both)
    }

    pub const fn applies_to_rust(&self) -> bool {
        matches!(self.applicability, Applicability::Rust | Applicability::Both)
    }
}

pub static RULES: &[Rule] = &[
    Rule {
        category: RuleCategory::Functions,
        template: "Keep functions ≤ {} statements",
        get_threshold: |c, _| c.statements_per_function,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Use ≤ {} positional arguments; prefer keyword-only args after that",
        get_threshold: |c, _| c.arguments_positional,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Limit keyword-only arguments to ≤ {}",
        get_threshold: |c, _| c.arguments_keyword_only,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Limit arguments to ≤ {}",
        get_threshold: |c, _| c.arguments_per_function,
        applicability: Applicability::Rust,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Keep indentation depth ≤ {} levels",
        get_threshold: |c, _| c.max_indentation_depth,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Limit branches to ≤ {} per function",
        get_threshold: |c, _| c.branches_per_function,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Keep local variables ≤ {} per function",
        get_threshold: |c, _| c.local_variables_per_function,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Limit return statements to ≤ {} per function",
        get_threshold: |c, _| c.returns_per_function,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Return ≤ {} values per return statement",
        get_threshold: |c, _| c.return_values_per_function,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Avoid deeply nested functions/closures (max depth: {})",
        get_threshold: |c, _| c.nested_function_depth,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Keep try blocks narrow (≤ {} statements)",
        get_threshold: |c, _| c.statements_per_try_block,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Limit boolean parameters to ≤ {} per function",
        get_threshold: |c, _| c.boolean_parameters,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Use ≤ {} decorators per function",
        get_threshold: |c, _| c.annotations_per_function,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Use ≤ {} attributes per function",
        get_threshold: |c, _| c.annotations_per_function,
        applicability: Applicability::Rust,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Keep calls per function ≤ {}",
        get_threshold: |c, _| c.calls_per_function,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Classes,
        template: "Keep methods per class/type ≤ {}",
        get_threshold: |c, _| c.methods_per_class,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Keep files ≤ {} statements",
        get_threshold: |c, _| c.statements_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Keep files ≤ {} functions",
        get_threshold: |c, _| c.functions_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Limit to ≤ {} concrete types per file",
        get_threshold: |c, _| c.concrete_types_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Keep imported names ≤ {} per file",
        get_threshold: |c, _| c.imported_names_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Dependencies,
        template: "Avoid circular dependencies",
        get_threshold: |_, _| 0,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Dependencies,
        template: "Keep cycles small (≤ {} modules)",
        get_threshold: |c, _| c.cycle_size,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Dependencies,
        template: "Limit transitive dependencies to ≤ {}",
        get_threshold: |c, _| c.transitive_dependencies,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Dependencies,
        template: "Keep dependency depth ≤ {}",
        get_threshold: |c, _| c.dependency_depth,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Testing,
        template: "Every function/class/type should be mentioned in a test file",
        get_threshold: |_, _| 0,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Testing,
        template: "Maintain ≥ {}% test reference coverage (static check: name must appear in a test file)",
        get_threshold: |_, g| g.test_coverage_threshold,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Duplication,
        template: "Avoid copy-pasted code blocks",
        get_threshold: |_, _| 0,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Duplication,
        template: "Factor out repeated patterns into shared functions",
        get_threshold: |_, _| 0,
        applicability: Applicability::Both,
    },
];

pub fn rules_for_python(config: &Config, gate: &GateConfig) -> Vec<(RuleCategory, Vec<String>)> {
    rules_grouped(config, gate, true)
}

pub fn rules_for_rust(config: &Config, gate: &GateConfig) -> Vec<(RuleCategory, Vec<String>)> {
    rules_grouped(config, gate, false)
}

fn rules_grouped(config: &Config, gate: &GateConfig, python: bool) -> Vec<(RuleCategory, Vec<String>)> {
    use RuleCategory::{Classes, Dependencies, Duplication, Files, Functions, Testing};
    let categories = [Functions, Classes, Files, Dependencies, Testing, Duplication];
    categories.iter().filter_map(|&cat| {
        let rules: Vec<String> = RULES.iter()
            .filter(|r| r.category == cat)
            .filter(|r| if python { r.applies_to_python() } else { r.applies_to_rust() })
            .map(|r| r.format(config, gate))
            .collect();
        if rules.is_empty() { None } else { Some((cat, rules)) }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rules_and_categories() {
        assert!(!RULES.is_empty());
        let config = Config::python_defaults();
        let gate = GateConfig::default();
        assert!(!rules_for_python(&config, &gate).is_empty());
        assert!(!rules_for_rust(&Config::rust_defaults(), &gate).is_empty());
        assert!(RULES[0].format(&config, &gate).contains(&config.statements_per_function.to_string()));
        assert_eq!(RuleCategory::Functions.python_heading(), "Functions");
        assert_eq!(RuleCategory::Classes.rust_heading(), "Types");
        assert_eq!(RuleCategory::Dependencies.python_heading(), "Dependencies");
        let _ = (Applicability::Python, Applicability::Rust, Applicability::Both);
        let rule = Rule { category: RuleCategory::Functions, template: "Test {}", get_threshold: |c, _| c.statements_per_function, applicability: Applicability::Both };
        assert!(rule.applies_to_python() && rule.applies_to_rust());
        let py_rule = Rule { category: RuleCategory::Functions, template: "Test", get_threshold: |_, _| 0, applicability: Applicability::Python };
        assert!(py_rule.applies_to_python() && !py_rule.applies_to_rust());
        let rs_rule = Rule { category: RuleCategory::Functions, template: "Test", get_threshold: |_, _| 0, applicability: Applicability::Rust };
        assert!(!rs_rule.applies_to_python() && rs_rule.applies_to_rust());
        assert!(!rules_grouped(&config, &gate, true).is_empty());
    }
}
