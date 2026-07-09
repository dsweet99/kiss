// Step 4 adds the policy engine before production planning is routed through it.
#![allow(dead_code)]

mod backer;
mod engine;
mod language_module;
mod types;

pub(crate) use backer::CoverageBacker;
pub(crate) use engine::CoverageDecisionEngine;
pub(crate) use language_module::RunContext;
pub(crate) use types::{
    ChangedDiff, ChangedSource, CoverageFreshness, PopulationPlan, SelectionDecision, TestSelector,
};
#[cfg(test)]
pub(crate) use types::{ChangedTestSelector, CoverageDecisionPlan};

#[cfg(test)]
#[path = "coverage_decision_test.rs"]
mod tests;
#[cfg(test)]
#[path = "coverage_decision_witness_test.rs"]
mod witness_tests;
