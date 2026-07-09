use kiss::Language;

use super::types::{
    ChangedDiff, CoverageFreshness, PopulationPlan, SelectionDecision, TestSelector,
};

type DiscoverFn = dyn Fn() -> Result<Vec<TestSelector>, String>;
type ChangedTestsFn = dyn Fn(&ChangedDiff) -> Vec<TestSelector>;
type PriorFailuresFn = dyn Fn() -> Vec<TestSelector>;
type FreshnessFn = dyn Fn(&[TestSelector]) -> Result<CoverageFreshness, String>;
type PopulationFn = dyn Fn(&[TestSelector]) -> PopulationPlan;
type SelectFn = dyn Fn() -> Result<SelectionDecision, String>;

pub(crate) struct CoverageBacker {
    language: Language,
    discover_test_universe: Box<DiscoverFn>,
    changed_tests: Box<ChangedTestsFn>,
    prior_failures: Box<PriorFailuresFn>,
    freshness: Box<FreshnessFn>,
    population_plan: Box<PopulationFn>,
    select: Box<SelectFn>,
    pub(crate) manifest_env_allowlist: &'static [&'static str],
}

impl CoverageBacker {
    pub(crate) fn new(
        language: Language,
        discover_test_universe: Box<DiscoverFn>,
        changed_tests: Box<ChangedTestsFn>,
        prior_failures: Box<PriorFailuresFn>,
        freshness: Box<FreshnessFn>,
        population_plan: Box<PopulationFn>,
        select: Box<SelectFn>,
    ) -> Self {
        Self {
            language,
            discover_test_universe,
            changed_tests,
            prior_failures,
            freshness,
            population_plan,
            select,
            manifest_env_allowlist: &[],
        }
    }

    pub(crate) fn language(&self) -> Language {
        self.language
    }

    pub(crate) fn discover_test_universe(&self) -> Result<Vec<TestSelector>, String> {
        (self.discover_test_universe)()
    }

    pub(crate) fn changed_tests(&self, diff: &ChangedDiff) -> Vec<TestSelector> {
        (self.changed_tests)(diff)
    }

    pub(crate) fn prior_failures(&self) -> Vec<TestSelector> {
        (self.prior_failures)()
    }

    pub(crate) fn freshness(&self, universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        (self.freshness)(universe)
    }

    pub(crate) fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        (self.population_plan)(universe)
    }

    pub(crate) fn select(&self) -> Result<SelectionDecision, String> {
        (self.select)()
    }

    pub(crate) fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        self.manifest_env_allowlist
    }
}
