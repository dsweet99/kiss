use super::types::{ChangedDiff, SelectionBasis, TestSelector};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use kiss::Language;

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
            fn sources_changed(&self) -> bool;
            fn selection_basis(&self) -> SelectionBasis {
                if self.sources_changed() {
                    SelectionBasis::Population
                } else {
                    SelectionBasis::Current
                }
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
            fn dry_run_lines(
                &self,
                selectors: &[String],
                population: bool,
                extra: &[String],
                jobs: usize,
            ) -> Result<Vec<String>, String>;
            fn stage_label(&self, population: bool) -> &'static str;
            fn population_from_plan(&self) -> bool {
                false
            }
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
