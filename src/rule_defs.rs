use crate::config::Config;
use crate::GateConfig;

#[derive(Clone, Copy)]
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
        get_threshold: |c, _| c.decorators_per_function,
        applicability: Applicability::Python,
    },
    Rule {
        category: RuleCategory::Functions,
        template: "Use ≤ {} attributes per function",
        get_threshold: |c, _| c.decorators_per_function,
        applicability: Applicability::Rust,
    },
    Rule {
        category: RuleCategory::Classes,
        template: "Keep methods per class/type ≤ {}",
        get_threshold: |c, _| c.methods_per_class,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Classes,
        template: "Ensure methods share fields (LCOM ≤ {}%)",
        get_threshold: |c, _| c.lcom,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Keep files ≤ {} lines",
        get_threshold: |c, _| c.lines_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Limit to ≤ {} classes/types per file",
        get_threshold: |c, _| c.classes_per_file,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Files,
        template: "Keep imports ≤ {} per file",
        get_threshold: |c, _| c.imports_per_file,
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
        template: "Limit fan-out (direct dependencies) to ≤ {}",
        get_threshold: |c, _| c.fan_out,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Dependencies,
        template: "Keep fan-in modules stable and well-tested (threshold: {})",
        get_threshold: |c, _| c.fan_in,
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
        template: "Every function/class/type should be referenced by tests",
        get_threshold: |_, _| 0,
        applicability: Applicability::Both,
    },
    Rule {
        category: RuleCategory::Testing,
        template: "Maintain ≥ {}% test reference coverage",
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
            .filter(|r| r.category as u8 == cat as u8)
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
    fn test_rules_registry_not_empty() {
        assert!(!RULES.is_empty());
    }

    #[test]
    fn test_rules_for_both_languages() {
        let config = Config::python_defaults();
        let gate = GateConfig::default();
        let py_rules = rules_for_python(&config, &gate);
        let rs_rules = rules_for_rust(&Config::rust_defaults(), &gate);
        assert!(!py_rules.is_empty());
        assert!(!rs_rules.is_empty());
    }

    #[test]
    fn test_rule_formatting() {
        let config = Config::python_defaults();
        let gate = GateConfig::default();
        let rule = &RULES[0];
        let formatted = rule.format(&config, &gate);
        assert!(formatted.contains(&config.statements_per_function.to_string()));
    }
}
