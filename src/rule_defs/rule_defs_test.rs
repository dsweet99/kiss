use crate::config::Config;
use crate::gate_config::GateConfig;

use super::grouping::rules_grouped;
use super::rules_table::RULES;
use super::types::{Applicability, Rule, RuleCategory};

#[test]
fn test_rules_and_categories() {
    assert!(!RULES.is_empty());
    let config = Config::python_defaults();
    let gate = GateConfig::default();
    assert!(!super::rules_for_python(&config, &gate).is_empty());
    assert!(!super::rules_for_rust(&Config::rust_defaults(), &gate).is_empty());
    assert!(
        RULES[0]
            .format(&config, &gate)
            .contains(&config.statements_per_function.to_string())
    );
    assert_eq!(RuleCategory::Functions.python_heading(), "Functions");
    assert_eq!(RuleCategory::Classes.rust_heading(), "Types");
    assert_eq!(RuleCategory::Dependencies.python_heading(), "Dependencies");
    let _ = (
        Applicability::Python,
        Applicability::Rust,
        Applicability::Both,
    );
    let rule = Rule {
        category: RuleCategory::Functions,
        template: "Test {}",
        get_threshold: |c, _| c.statements_per_function,
        applicability: Applicability::Both,
    };
    assert!(rule.applies_to_python() && rule.applies_to_rust());
    let py_rule = Rule {
        category: RuleCategory::Functions,
        template: "Test",
        get_threshold: |_, _| 0,
        applicability: Applicability::Python,
    };
    assert!(py_rule.applies_to_python() && !py_rule.applies_to_rust());
    let rs_rule = Rule {
        category: RuleCategory::Functions,
        template: "Test",
        get_threshold: |_, _| 0,
        applicability: Applicability::Rust,
    };
    assert!(!rs_rule.applies_to_python() && rs_rule.applies_to_rust());
    assert!(!rules_grouped(&config, &gate, true).is_empty());
}
