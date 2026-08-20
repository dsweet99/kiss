mod grouping;
mod rules_table;
mod types;

pub use grouping::{rules_for_python, rules_for_rust};
pub use rules_table::RULES;
pub use types::{Applicability, Rule, RuleCategory};

#[cfg(test)]
#[path = "rule_defs_test.rs"]
mod rule_defs_test;
