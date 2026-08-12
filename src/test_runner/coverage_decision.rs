#[cfg(test)]
mod changed_test_selector;
mod engine;
mod language_module;
mod types;

#[cfg(test)]
pub(crate) use changed_test_selector::ChangedTestSelector;
pub(crate) use engine::CoverageDecisionEngine;
pub(crate) use language_module::{
    LanguageExecutor, LanguagePlanner, LanguageTestModule, RunContext, SupportedLanguage,
};
#[cfg(test)]
pub(crate) use types::CoverageDecisionPlan;
pub(crate) use types::{
    ChangedDiff, ChangedSource, CoverageFreshness, PopulationPlan, RustSelectionBasis,
    SelectionDecision, TestSelector, full_population_plan,
};

#[cfg(test)]
#[path = "coverage_decision_test.rs"]
mod tests;
#[cfg(test)]
#[path = "coverage_decision_witness_test.rs"]
mod witness_tests;
