use super::types::{
    ChangedDiff, CoverageFreshness, PopulationPlan, SelectionBasis, SelectionDecision,
    TestSelector,
};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use kiss::Language;
use std::path::Path;

/// Shared language-identity concept for planner/executor and ensure runtime stacks (#6).
///
/// Planner and ensure `LanguageRuntime` types implement this in addition to their
/// stack-specific traits so language identity is not redefined per abstraction.
pub(crate) trait SupportedLanguage {
    fn language(&self) -> Language;
}

macro_rules! define_language_policy_traits {
    () => {
        pub(crate) trait LanguagePlanner {
            fn language(&self) -> Language;
            fn discover_universe(&self) -> Result<Vec<TestSelector>, String>;
            fn changed_tests(&self, diff: &ChangedDiff) -> Vec<TestSelector>;
            fn prior_failures(&self) -> Vec<TestSelector>;
            fn freshness(&self, universe: &[TestSelector]) -> Result<CoverageFreshness, String>;
            fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan;
            fn select(&self) -> Result<SelectionDecision, String>;
            fn manifest_env_allowlist(&self) -> &'static [&'static str];
            /// How this language chose covering tests (symmetric for Python and Rust).
            fn selection_basis(&self) -> SelectionBasis {
                SelectionBasis::Current
            }
        }

        pub(crate) trait LanguageExecutor {
            fn language(&self) -> Language;
            fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool;
            fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String>;
            fn run_population(
                &self,
                selectors: &[String],
                ctx: &RunContext<'_, '_>,
            ) -> Result<SelectorExecutionSummary, String>;
            fn run_selective(
                &self,
                selectors: &[String],
                ctx: &RunContext<'_, '_>,
            ) -> Result<SelectorExecutionSummary, String>;
            fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String>;
            fn write_manifest(
                &self,
                selectors: &[String],
                ctx: &RunContext<'_, '_>,
            ) -> Result<(), String>;
            fn is_indexable_source(&self, path: &Path, repo_root: &Path) -> bool;
            fn dry_run_lines(
                &self,
                selectors: &[String],
                population: bool,
                extra: &[String],
                jobs: usize,
            ) -> Result<Vec<String>, String>;
        }

        pub(crate) trait LanguageTestModule: LanguagePlanner + LanguageExecutor {}

        impl<T> LanguageTestModule for T where T: LanguagePlanner + LanguageExecutor {}
    };
}

define_language_policy_traits!();

pub(crate) struct RunContext<'a, 'b> {
    pub(crate) planned: &'a PlannedSelectors,
    pub(crate) options: &'a SelectorRunOptions<'b>,
}
