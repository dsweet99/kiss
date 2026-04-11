//! Human-friendly rule definitions for library consumers.
//!
//! This module provides categorized rules with sentence-style templates suitable
//! for display in documentation or UIs. For machine-readable rule output (RULE: lines),
//! see the `rules` module in the binary crate which outputs structured specs for LLM consumption.
//!
//! Both modules now use canonical metric IDs to ensure consistency.

mod grouping;
mod rules_table;
mod types;

pub use grouping::{rules_for_python, rules_for_rust};
pub use rules_table::RULES;
pub use types::{Applicability, Rule, RuleCategory};

#[cfg(test)]
#[path = "rule_defs_test.rs"]
mod rule_defs_test;
