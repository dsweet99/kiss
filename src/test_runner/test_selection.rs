#[cfg(test)]
mod changed_test_selector;
mod engine;
mod language_module;
mod types;

#[cfg(test)]
pub(crate) use changed_test_selector::ChangedTestSelector;
pub(crate) use engine::SelectionEngine;
pub(crate) use language_module::{
    LanguageExecutor, LanguagePlanner, LanguageTestModule, RunContext, SupportedLanguage,
};
#[cfg(test)]
pub(crate) use types::SelectionPlan;
pub(crate) use types::{ChangedDiff, ChangedSource, SelectionBasis, TestSelector};

#[cfg(test)]
#[path = "test_selection_test.rs"]
mod tests;
#[cfg(test)]
#[path = "test_selection_witness_test.rs"]
mod witness_tests;
