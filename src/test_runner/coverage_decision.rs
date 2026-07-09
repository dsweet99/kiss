mod engine;
mod language_module;
mod types;

pub(crate) use engine::CoverageDecisionEngine;
pub(crate) use language_module::{
    LanguageExecutor, LanguagePlanner, LanguageTestModule, RunContext,
};
pub(crate) use types::{
    ChangedDiff, ChangedSource, CoverageFreshness, PopulationPlan, SelectionDecision, TestSelector,
    full_population_plan,
};
#[cfg(test)]
pub(crate) use types::{ChangedTestSelector, CoverageDecisionPlan};

#[cfg(test)]
#[path = "coverage_decision_test.rs"]
mod tests;
#[cfg(test)]
#[path = "coverage_decision_witness_test.rs"]
mod witness_tests;
