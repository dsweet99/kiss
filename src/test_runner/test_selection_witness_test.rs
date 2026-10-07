use super::{
    ChangedDiff, ChangedSource, ChangedTestSelector, LanguagePlanner, SelectionBasis,
    SelectionEngine, SelectionPlan, TestSelector,
};
use kiss::Language;

fn selector(language: Language, id: &str) -> TestSelector {
    TestSelector::new(language, id)
}

struct StaticPlanner {
    language: Language,
    universe: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
    sources_changed: bool,
}

impl LanguagePlanner for StaticPlanner {
    fn language(&self) -> Language {
        self.language
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        Ok(self.universe.clone())
    }

    fn changed_tests(&self, _diff: &ChangedDiff) -> Vec<TestSelector> {
        Vec::new()
    }

    fn prior_failures(&self) -> Vec<TestSelector> {
        self.prior_failures.clone()
    }

    fn sources_changed(&self) -> bool {
        self.sources_changed
    }
}

#[test]
fn witness_changed_policy_types() {
    let selector = TestSelector::new(Language::Rust, "crate::tests::works");
    let changed_test = ChangedTestSelector::new(selector.clone());
    let source = ChangedSource::new(Language::Rust, "src/lib.rs");
    let diff = ChangedDiff::new(vec![source.clone()]);

    assert_eq!(changed_test.selector, selector);
    assert_eq!(source.path, "src/lib.rs");
    assert_eq!(diff.sources, vec![source]);
    assert_eq!(diff.sources_for_language(Language::Rust).len(), 1);
    assert!(diff.sources_for_language(Language::Python).is_empty());
}

#[test]
fn witness_plan_defaults_and_debug() {
    let selector = TestSelector::new(Language::Rust, "crate::tests::works");
    let plan = SelectionPlan {
        selected: vec![selector.clone()],
        population: vec![selector],
        population_languages: vec![Language::Rust],
    };

    assert_eq!(plan.selected, plan.population);
    assert_eq!(plan.population_languages, vec![Language::Rust]);
    assert!(SelectionPlan::default().selected.is_empty());
    assert!(format!("{:?}", plan.clone()).contains("selected"));
    assert_eq!(SelectionBasis::default(), SelectionBasis::Current);
    assert_ne!(SelectionBasis::Current, SelectionBasis::Population);
}

#[test]
fn prior_failures_are_selected_without_source_changes() {
    let prior_failure = selector(Language::Python, "tests/test_app.py::test_failed");
    let planner = StaticPlanner {
        language: Language::Python,
        universe: vec![prior_failure.clone()],
        prior_failures: vec![prior_failure.clone()],
        sources_changed: false,
    };

    let plan = SelectionEngine::new(vec![Box::new(planner)])
        .plan(&[])
        .unwrap();

    assert_eq!(plan.selected, vec![prior_failure]);
    assert!(plan.population.is_empty());
}

#[test]
fn prior_failures_join_the_population_when_sources_change() {
    let prior_failure = selector(Language::Rust, "crate::tests::previously_failed");
    let planner = StaticPlanner {
        language: Language::Rust,
        universe: vec![
            selector(Language::Rust, "crate::tests::other"),
            prior_failure.clone(),
        ],
        prior_failures: vec![prior_failure.clone()],
        sources_changed: true,
    };

    let plan = SelectionEngine::new(vec![Box::new(planner)])
        .plan(&[ChangedSource::new(Language::Rust, "src/lib.rs")])
        .unwrap();

    assert_eq!(
        plan.population,
        vec![
            selector(Language::Rust, "crate::tests::other"),
            prior_failure
        ]
    );
    assert!(plan.selected.is_empty());
}
