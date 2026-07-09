use super::backer::CoverageBacker;
use super::types::{
    ChangedDiff, CoverageFreshness, PopulationPlan, SelectionDecision, TestSelector,
};
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use kiss::Language;

pub(crate) struct LanguagePlanner {
    backer: CoverageBacker,
}

impl LanguagePlanner {
    pub(crate) fn new(backer: CoverageBacker) -> Self {
        Self { backer }
    }

    pub(crate) fn language(&self) -> Language {
        self.backer.language()
    }

    pub(crate) fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        self.backer.discover_test_universe()
    }

    pub(crate) fn changed_tests(&self, diff: &ChangedDiff) -> Vec<TestSelector> {
        self.backer.changed_tests(diff)
    }

    pub(crate) fn prior_failures(&self) -> Vec<TestSelector> {
        self.backer.prior_failures()
    }

    pub(crate) fn freshness(&self, universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        self.backer.freshness(universe)
    }

    pub(crate) fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        self.backer.population_plan(universe)
    }

    pub(crate) fn select(&self) -> Result<SelectionDecision, String> {
        self.backer.select()
    }

    pub(crate) fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        self.backer.manifest_env_allowlist()
    }
}

pub(crate) struct RunContext<'a, 'b> {
    pub(crate) planned: &'a PlannedSelectors,
    pub(crate) options: &'a SelectorRunOptions<'b>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::coverage_decision::{
        ChangedDiff, ChangedSource, CoverageBacker, CoverageFreshness, PopulationPlan,
        SelectionDecision, TestSelector,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn language_planner_forwards_all_policy_methods() {
        let select_seen = Rc::new(Cell::new(false));
        let select_seen_for_closure = Rc::clone(&select_seen);
        let universe = vec![TestSelector::new(
            Language::Python,
            "tests/test_app.py::test_app",
        )];
        let changed = TestSelector::new(Language::Python, "tests/test_app.py::test_changed");
        let prior = TestSelector::new(Language::Python, "tests/test_app.py::test_prior");
        let selected = TestSelector::new(Language::Python, "tests/test_app.py::test_selected");
        let planner = LanguagePlanner::new(CoverageBacker::new(
            Language::Python,
            Box::new({
                let universe = universe.clone();
                move || Ok(universe.clone())
            }),
            Box::new({
                let changed = changed.clone();
                move |_diff: &ChangedDiff| vec![changed.clone()]
            }),
            Box::new({
                let prior = prior.clone();
                move || vec![prior.clone()]
            }),
            Box::new(|_universe| Ok(CoverageFreshness::Fresh)),
            Box::new(|universe| PopulationPlan {
                selectors: universe.to_vec(),
            }),
            Box::new(move || {
                select_seen_for_closure.set(true);
                Ok(SelectionDecision {
                    selectors: vec![selected.clone()],
                    complete: true,
                })
            }),
        ));
        let diff = ChangedDiff::new(vec![ChangedSource::new(Language::Python, "app.py")]);

        assert_eq!(planner.language(), Language::Python);
        assert_eq!(planner.discover_universe().unwrap(), universe);
        assert_eq!(planner.changed_tests(&diff), vec![changed]);
        assert_eq!(planner.prior_failures(), vec![prior]);
        assert_eq!(
            planner.freshness(&universe).unwrap(),
            CoverageFreshness::Fresh
        );
        assert_eq!(planner.population_plan(&universe).selectors, universe);
        assert!(planner.select().unwrap().complete);
        assert!(select_seen.get());
        assert!(planner.manifest_env_allowlist().is_empty());
    }

    #[test]
    fn language_planner_policy_surface_is_explicit() {
        let universe = vec![TestSelector::new(
            Language::Rust,
            "crate::tests::test_policy",
        )];
        let planner = LanguagePlanner::new(CoverageBacker::new(
            Language::Rust,
            Box::new({
                let universe = universe.clone();
                move || Ok(universe.clone())
            }),
            Box::new(|_diff: &ChangedDiff| Vec::new()),
            Box::new(Vec::new),
            Box::new(|_universe| Ok(CoverageFreshness::Fresh)),
            Box::new(|universe| PopulationPlan {
                selectors: universe.to_vec(),
            }),
            Box::new(|| Ok(SelectionDecision::default())),
        ));

        assert_eq!(planner.language(), Language::Rust);
        assert_eq!(planner.discover_universe().unwrap(), universe);
    }
}
