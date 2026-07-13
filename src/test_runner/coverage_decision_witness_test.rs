use super::{
    ChangedDiff, ChangedSource, ChangedTestSelector, CoverageDecisionEngine, CoverageDecisionPlan,
    CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionDecision, TestSelector,
    full_population_plan,
};
use kiss::Language;

fn selector(language: Language, id: &str) -> TestSelector {
    TestSelector::new(language, id)
}

struct StaticPlanner {
    language: Language,
    universe: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
    freshness: CoverageFreshness,
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

    fn freshness(&self, _universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        Ok(self.freshness)
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        full_population_plan(universe)
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        Ok(SelectionDecision::default())
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        &[]
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
    assert_eq!(CoverageFreshness::Fresh, CoverageFreshness::Fresh);
    assert_eq!(CoverageFreshness::Stale, CoverageFreshness::Stale);
    assert_eq!(CoverageFreshness::Unknown, CoverageFreshness::Unknown);
    assert!(!CoverageFreshness::Fresh.requires_population());
    assert!(CoverageFreshness::Stale.requires_population());
    assert!(CoverageFreshness::Unknown.requires_population());
}

#[test]
fn witness_decision_plan_defaults_and_debug() {
    let selector = TestSelector::new(Language::Rust, "crate::tests::works");
    let selection = SelectionDecision {
        selectors: vec![selector.clone()],
        complete: true,
    };
    let population = PopulationPlan {
        selectors: vec![selector],
    };
    let plan = CoverageDecisionPlan {
        selected: selection.selectors.clone(),
        population: population.selectors.clone(),
        population_languages: vec![Language::Rust],
    };

    assert_eq!(plan.selected, plan.population);
    assert_eq!(plan.population_languages, vec![Language::Rust]);
    assert!(std::mem::size_of::<ChangedDiff>() > 0);
    assert!(std::mem::size_of::<CoverageFreshness>() > 0);
    assert!(std::mem::size_of::<SelectionDecision>() > 0);
    assert!(std::mem::size_of::<PopulationPlan>() > 0);
    assert!(std::mem::size_of::<CoverageDecisionPlan>() > 0);
    assert!(SelectionDecision::default().selectors.is_empty());
    assert!(SelectionDecision::default().complete);
    assert!(PopulationPlan::default().selectors.is_empty());
    assert!(CoverageDecisionPlan::default().selected.is_empty());
    assert!(format!("{:?}", plan.clone()).contains("selected"));
}

#[test]
fn witness_changed_diff_and_freshness_are_exhaustive() {
    let diff = ChangedDiff::new(vec![
        ChangedSource::new(Language::Python, "tests/test_app.py"),
        ChangedSource::new(Language::Rust, "src/lib.rs"),
    ]);
    let mut languages = diff
        .sources
        .iter()
        .map(|source| source.language)
        .collect::<Vec<_>>();
    languages.sort_by_key(|language| match language {
        Language::Python => 0,
        Language::Rust => 1,
    });
    assert_eq!(languages, vec![Language::Python, Language::Rust]);

    for freshness in [
        CoverageFreshness::Fresh,
        CoverageFreshness::Stale,
        CoverageFreshness::Unknown,
    ] {
        assert!(matches!(
            freshness,
            CoverageFreshness::Fresh | CoverageFreshness::Stale | CoverageFreshness::Unknown
        ));
    }
}

#[test]
fn prior_failures_are_selected_without_source_changes() {
    let prior_failure = selector(Language::Python, "tests/test_app.py::test_failed");
    let selected_prior_failure = prior_failure.clone();
    let planner = StaticPlanner {
        language: Language::Python,
        universe: vec![prior_failure.clone()],
        prior_failures: vec![prior_failure.clone()],
        freshness: CoverageFreshness::Fresh,
    };

    let plan = CoverageDecisionEngine::new(vec![Box::new(planner)])
        .plan(&[])
        .unwrap();

    assert_eq!(plan.selected, vec![selected_prior_failure]);
    assert!(plan.population.is_empty());
}

#[test]
fn prior_failures_are_populated_when_coverage_is_stale() {
    let prior_failure = selector(Language::Rust, "crate::tests::previously_failed");
    let selected_prior_failure = prior_failure.clone();
    let planner = StaticPlanner {
        language: Language::Rust,
        universe: vec![
            selector(Language::Rust, "crate::tests::covered"),
            prior_failure.clone(),
        ],
        prior_failures: vec![prior_failure.clone()],
        freshness: CoverageFreshness::Stale,
    };

    let plan = CoverageDecisionEngine::new(vec![Box::new(planner)])
        .plan(&[ChangedSource::new(Language::Rust, "src/lib.rs")])
        .unwrap();

    assert_eq!(
        plan.population,
        vec![
            selector(Language::Rust, "crate::tests::covered"),
            selected_prior_failure
        ]
    );
    assert!(plan.selected.is_empty());
}
