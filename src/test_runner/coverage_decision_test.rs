use super::{
    ChangedDiff, ChangedSource, CoverageDecisionEngine, CoverageFreshness, LanguagePlanner,
    PopulationPlan, SelectionDecision, TestSelector, full_population_plan,
};
use kiss::Language;
use std::cell::Cell;
use std::rc::Rc;

struct FakePlanner {
    language: Language,
    universe: Vec<TestSelector>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
    freshness: CoverageFreshness,
    population: Vec<TestSelector>,
    selected: Vec<TestSelector>,
    selection_complete: bool,
    select_calls: Rc<Cell<usize>>,
}

impl FakePlanner {
    fn fresh(language: Language, selected: Vec<TestSelector>) -> (Self, Rc<Cell<usize>>) {
        let select_calls = Rc::new(Cell::new(0));
        (
            Self {
                language,
                universe: vec![selector(language, "a"), selector(language, "b")],
                changed_tests: vec![],
                prior_failures: vec![],
                freshness: CoverageFreshness::Fresh,
                population: vec![selector(language, "a"), selector(language, "b")],
                selected,
                selection_complete: true,
                select_calls: Rc::clone(&select_calls),
            },
            select_calls,
        )
    }

    fn boxed(self) -> Box<dyn LanguagePlanner> {
        Box::new(self)
    }
}

impl LanguagePlanner for FakePlanner {
    fn language(&self) -> Language {
        self.language
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        Ok(self.universe.clone())
    }

    fn changed_tests(&self, diff: &ChangedDiff) -> Vec<TestSelector> {
        if !diff.sources.is_empty() {
            assert!(
                diff.sources
                    .iter()
                    .any(|source| source.language == self.language)
            );
        }
        self.changed_tests.clone()
    }

    fn prior_failures(&self) -> Vec<TestSelector> {
        self.prior_failures.clone()
    }

    fn freshness(&self, universe: &[TestSelector]) -> Result<CoverageFreshness, String> {


        if !universe.is_empty() {
            assert_eq!(universe, self.universe.as_slice());
        }
        Ok(self.freshness)
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        assert_eq!(universe, self.universe.as_slice());
        PopulationPlan {
            selectors: self.population.clone(),
        }
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        self.select_calls.set(self.select_calls.get() + 1);
        Ok(SelectionDecision {
            selectors: self.selected.clone(),
            complete: self.selection_complete,
        })
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        &[]
    }
}

fn selector(language: Language, id: &str) -> TestSelector {
    TestSelector::new(language, id)
}

fn source(language: Language, path: &str) -> ChangedSource {
    ChangedSource::new(language, path)
}

#[test]
fn fake_planner_exposes_trait_policy() {
    let universe = vec![selector(Language::Rust, "crate::tests::works")];
    let planner = FakePlanner {
        language: Language::Rust,
        universe: universe.clone(),
        changed_tests: Vec::new(),
        prior_failures: Vec::new(),
        freshness: CoverageFreshness::Fresh,
        population: Vec::new(),
        selected: Vec::new(),
        selection_complete: true,
        select_calls: Rc::new(Cell::new(0)),
    };

    assert_eq!(planner.language(), Language::Rust);
    assert_eq!(planner.discover_universe().unwrap(), universe);
    assert_eq!(
        planner.freshness(&planner.universe).unwrap(),
        CoverageFreshness::Fresh
    );
    assert!(
        planner
            .changed_tests(&ChangedDiff::new(Vec::new()))
            .is_empty()
    );
}

#[test]
#[allow(non_snake_case)]
fn CoverageFreshness_and_full_population_plan_contracts() {
    let selector = selector(Language::Python, "tests/test_app.py::test_value");
    let plan = full_population_plan(std::slice::from_ref(&selector));

    assert_eq!(plan.selectors, vec![selector]);
    assert!(!CoverageFreshness::Fresh.requires_population());
    assert!(!CoverageFreshness::ReusablePrior.requires_population());
    assert!(CoverageFreshness::Stale.requires_population());
    assert!(CoverageFreshness::Unknown.requires_population());
}

#[test]
fn test_selector_ordering_groups_python_before_rust_and_selection_defaults_complete() {
    let mut selectors = vec![
        selector(Language::Rust, "crate::tests::b"),
        selector(Language::Python, "tests/test_app.py::test_b"),
        selector(Language::Python, "tests/test_app.py::test_a"),
        selector(Language::Rust, "crate::tests::a"),
    ];
    selectors.sort();
    assert_eq!(
        selectors,
        vec![
            selector(Language::Python, "tests/test_app.py::test_a"),
            selector(Language::Python, "tests/test_app.py::test_b"),
            selector(Language::Rust, "crate::tests::a"),
            selector(Language::Rust, "crate::tests::b"),
        ]
    );

    let decision = SelectionDecision::default();
    assert!(decision.complete);
    assert!(decision.selectors.is_empty());
}

#[test]
fn reusable_prior_backer_selects_without_population() {
    let (planner, select_calls) =
        FakePlanner::fresh(Language::Rust, vec![selector(Language::Rust, "b")]);
    let mut planner = planner;
    planner.freshness = CoverageFreshness::ReusablePrior;
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib.rs")])
        .unwrap();
    assert_eq!(plan.selected, vec![selector(Language::Rust, "b")]);
    assert!(plan.population.is_empty());
    assert_eq!(select_calls.get(), 1);
}

#[test]
fn fresh_backer_selects_affected_tests() {
    let (planner, select_calls) =
        FakePlanner::fresh(Language::Rust, vec![selector(Language::Rust, "b")]);
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib.rs")])
        .unwrap();
    assert_eq!(plan.selected, vec![selector(Language::Rust, "b")]);
    assert!(plan.population.is_empty());
    assert_eq!(select_calls.get(), 1);
}

#[test]
fn prior_failures_outside_pytest_universe_are_dropped() {
    let universe = vec![selector(Language::Python, "tests/test_app.py::test_ok")];
    let planner = FakePlanner {
        language: Language::Python,
        universe: universe.clone(),
        changed_tests: vec![],
        prior_failures: vec![selector(
            Language::Python,
            "/abs/tests/fixtures/mv/python/test.py::test_stale",
        )],
        freshness: CoverageFreshness::Fresh,
        population: universe,
        selected: vec![selector(Language::Python, "tests/test_app.py::test_ok")],
        selection_complete: true,
        select_calls: Rc::new(Cell::new(0)),
    };
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[])
        .unwrap();
    assert_eq!(
        plan.selected,
        vec![selector(Language::Python, "tests/test_app.py::test_ok")]
    );
}

#[test]
fn fresh_incomplete_selection_escalates_to_population() {
    let select_calls = Rc::new(Cell::new(0));
    let planner = FakePlanner {
        language: Language::Python,
        universe: vec![
            selector(Language::Python, "a"),
            selector(Language::Python, "b"),
            selector(Language::Python, "changed"),
            selector(Language::Python, "failed"),
        ],
        changed_tests: vec![selector(Language::Python, "changed")],
        prior_failures: vec![selector(Language::Python, "failed")],
        freshness: CoverageFreshness::Fresh,
        population: vec![
            selector(Language::Python, "a"),
            selector(Language::Python, "b"),
        ],
        selected: Vec::new(),
        selection_complete: false,
        select_calls: Rc::clone(&select_calls),
    };

    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Python, "src/app.py")])
        .unwrap();

    assert_eq!(
        plan.population,
        vec![
            selector(Language::Python, "a"),
            selector(Language::Python, "b"),
            selector(Language::Python, "changed"),
            selector(Language::Python, "failed"),
        ]
    );
    assert_eq!(plan.population_languages, vec![Language::Python]);
    assert!(plan.selected.is_empty());
    assert_eq!(select_calls.get(), 1);
}

#[test]
fn stale_backer_returns_population_plan_without_selecting() {
    let select_calls = Rc::new(Cell::new(0));
    let planner = FakePlanner {
        language: Language::Rust,
        universe: vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")],
        changed_tests: vec![],
        prior_failures: vec![],
        freshness: CoverageFreshness::Stale,
        population: vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")],
        selected: vec![selector(Language::Rust, "b")],
        selection_complete: true,
        select_calls: Rc::clone(&select_calls),
    };
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib.rs")])
        .unwrap();
    assert_eq!(
        plan.population,
        vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")]
    );
    assert!(plan.selected.is_empty());
    assert_eq!(select_calls.get(), 0);
}

#[test]
fn unknown_freshness_returns_population_plan() {
    let select_calls = Rc::new(Cell::new(0));
    let planner = FakePlanner {
        language: Language::Python,
        universe: vec![selector(Language::Python, "a")],
        changed_tests: vec![],
        prior_failures: vec![],
        freshness: CoverageFreshness::Unknown,
        population: vec![selector(Language::Python, "a")],
        selected: vec![],
        selection_complete: true,
        select_calls,
    };
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Python, "src/app.py")])
        .unwrap();
    assert_eq!(plan.population, vec![selector(Language::Python, "a")]);
    assert!(plan.selected.is_empty());
}

#[test]
fn changed_test_selectors_and_population_selectors_are_deduped() {
    let select_calls = Rc::new(Cell::new(0));
    let planner = FakePlanner {
        language: Language::Rust,
        universe: vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")],
        changed_tests: vec![selector(Language::Rust, "a"), selector(Language::Rust, "a")],
        prior_failures: vec![],
        freshness: CoverageFreshness::Stale,
        population: vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")],
        selected: vec![],
        selection_complete: true,
        select_calls,
    };
    let plan = CoverageDecisionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib_test.rs")])
        .unwrap();
    assert_eq!(
        plan.population,
        vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")]
    );
    assert!(plan.selected.is_empty());
}

#[test]
fn multiple_language_backers_combine_without_selector_collisions() {
    let (rust_planner, _rust_calls) =
        FakePlanner::fresh(Language::Rust, vec![selector(Language::Rust, "same")]);
    let (python_planner, _python_calls) =
        FakePlanner::fresh(Language::Python, vec![selector(Language::Python, "same")]);
    let plan = CoverageDecisionEngine::new(vec![rust_planner.boxed(), python_planner.boxed()])
        .plan(&[
            source(Language::Rust, "src/lib.rs"),
            source(Language::Python, "app.py"),
        ])
        .unwrap();
    assert_eq!(
        plan.selected,
        vec![
            selector(Language::Python, "same"),
            selector(Language::Rust, "same")
        ]
    );
    assert!(plan.population.is_empty());
}

#[test]
fn supported_language_unifies_planner_and_runtime_stacks() {
    use crate::test_runner::coverage_decision::SupportedLanguage;
    use crate::test_runner::lang_iface::LanguageRuntime;
    use crate::test_runner::lang_python::PythonRuntime;
    use crate::test_runner::lang_rust::RustRuntime;
    use kiss::Language;

    assert_eq!(
        <PythonRuntime as SupportedLanguage>::language(&PythonRuntime),
        Language::Python
    );
    assert_eq!(
        <RustRuntime as SupportedLanguage>::language(&RustRuntime),
        Language::Rust
    );
    assert_eq!(LanguageRuntime::language(&PythonRuntime), Language::Python);
    assert_eq!(LanguageRuntime::language(&RustRuntime), Language::Rust);
}
