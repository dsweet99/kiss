use super::{
    ChangedDiff, ChangedSource, CoverageBacker, CoverageDecisionEngine, CoverageFreshness,
    PopulationPlan, SelectionDecision, TestSelector,
};
use kiss::Language;
use std::cell::Cell;
use std::rc::Rc;

type DiscoverFn = Box<dyn Fn() -> Result<Vec<TestSelector>, String>>;
type ChangedTestsFn = Box<dyn Fn(&ChangedDiff) -> Vec<TestSelector>>;
type PriorFailuresFn = Box<dyn Fn() -> Vec<TestSelector>>;
type FreshnessFn = Box<dyn Fn(&[TestSelector]) -> Result<CoverageFreshness, String>>;
type PopulationFn = Box<dyn Fn(&[TestSelector]) -> PopulationPlan>;
type SelectFn = Box<dyn Fn() -> Result<SelectionDecision, String>>;

struct FakeBacker {
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

impl FakeBacker {
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

    fn into_backer(self) -> CoverageBacker {
        let language = self.language;
        CoverageBacker::new(
            language,
            fake_discover(self.universe.clone()),
            fake_changed_tests(language, self.changed_tests),
            fake_prior_failures(self.prior_failures),
            fake_freshness(self.universe.clone(), self.freshness),
            fake_population(self.universe, self.population),
            fake_select(
                Rc::clone(&self.select_calls),
                self.selected,
                self.selection_complete,
            ),
        )
    }
}

fn fake_discover(universe: Vec<TestSelector>) -> DiscoverFn {
    Box::new(move || Ok(universe.clone()))
}

fn fake_changed_tests(language: Language, changed_tests: Vec<TestSelector>) -> ChangedTestsFn {
    Box::new(move |diff: &ChangedDiff| {
        let changed_languages = diff
            .sources
            .iter()
            .map(|source| source.language)
            .collect::<Vec<_>>();
        assert!(changed_languages.contains(&language));
        changed_tests.clone()
    })
}

fn fake_prior_failures(prior_failures: Vec<TestSelector>) -> PriorFailuresFn {
    Box::new(move || prior_failures.clone())
}

fn fake_freshness(universe: Vec<TestSelector>, freshness: CoverageFreshness) -> FreshnessFn {
    Box::new(move |actual: &[TestSelector]| {
        assert_eq!(actual, universe.as_slice());
        Ok(freshness)
    })
}

fn fake_population(universe: Vec<TestSelector>, population: Vec<TestSelector>) -> PopulationFn {
    Box::new(move |actual: &[TestSelector]| {
        assert_eq!(actual, universe.as_slice());
        PopulationPlan {
            selectors: population.clone(),
        }
    })
}

fn fake_select(
    select_calls: Rc<Cell<usize>>,
    selected: Vec<TestSelector>,
    complete: bool,
) -> SelectFn {
    Box::new(move || {
        select_calls.set(select_calls.get() + 1);
        Ok(SelectionDecision {
            selectors: selected.clone(),
            complete,
        })
    })
}

fn selector(language: Language, id: &str) -> TestSelector {
    TestSelector::new(language, id)
}

fn source(language: Language, path: &str) -> ChangedSource {
    ChangedSource::new(language, path)
}

struct ForwardingFixture {
    backer: CoverageBacker,
    universe: Vec<TestSelector>,
    changed_test: TestSelector,
    selected: TestSelector,
    changed_seen: Rc<Cell<bool>>,
    freshness_seen: Rc<Cell<bool>>,
    population_seen: Rc<Cell<bool>>,
    select_seen: Rc<Cell<bool>>,
}

fn forwarding_fixture() -> ForwardingFixture {
    let universe = vec![selector(Language::Python, "tests/test_app.py::test_app")];
    let changed_test = selector(Language::Python, "tests/test_app.py::test_changed");
    let selected = selector(Language::Python, "tests/test_app.py::test_selected");
    let changed_seen = Rc::new(Cell::new(false));
    let freshness_seen = Rc::new(Cell::new(false));
    let population_seen = Rc::new(Cell::new(false));
    let select_seen = Rc::new(Cell::new(false));
    let backer = CoverageBacker::new(
        Language::Python,
        discover_callback(universe.clone()),
        changed_tests_callback(Rc::clone(&changed_seen), changed_test.clone()),
        prior_failures_callback(),
        freshness_callback(Rc::clone(&freshness_seen), universe.clone()),
        population_callback(Rc::clone(&population_seen), universe.clone()),
        select_callback(Rc::clone(&select_seen), selected.clone()),
    );
    ForwardingFixture {
        backer,
        universe,
        changed_test,
        selected,
        changed_seen,
        freshness_seen,
        population_seen,
        select_seen,
    }
}

fn discover_callback(universe: Vec<TestSelector>) -> DiscoverFn {
    Box::new(move || Ok(universe.clone()))
}

fn changed_tests_callback(seen: Rc<Cell<bool>>, changed_test: TestSelector) -> ChangedTestsFn {
    Box::new(move |diff: &ChangedDiff| {
        seen.set(true);
        assert_eq!(diff.sources_for_language(Language::Python).len(), 1);
        vec![changed_test.clone()]
    })
}

fn prior_failures_callback() -> PriorFailuresFn {
    Box::new(Vec::new)
}

fn freshness_callback(seen: Rc<Cell<bool>>, universe: Vec<TestSelector>) -> FreshnessFn {
    Box::new(move |actual: &[TestSelector]| {
        seen.set(true);
        assert_eq!(actual, universe.as_slice());
        Ok(CoverageFreshness::Fresh)
    })
}

fn population_callback(seen: Rc<Cell<bool>>, universe: Vec<TestSelector>) -> PopulationFn {
    Box::new(move |actual: &[TestSelector]| {
        seen.set(true);
        assert_eq!(actual, universe.as_slice());
        PopulationPlan {
            selectors: actual.to_vec(),
        }
    })
}

fn select_callback(seen: Rc<Cell<bool>>, selected: TestSelector) -> SelectFn {
    Box::new(move || {
        seen.set(true);
        Ok(SelectionDecision {
            selectors: vec![selected.clone()],
            complete: true,
        })
    })
}

#[test]
fn coverage_backer_new_sets_language_and_callbacks() {
    let universe = vec![selector(Language::Rust, "crate::tests::works")];
    let backer = CoverageBacker::new(
        Language::Rust,
        Box::new({
            let universe = universe.clone();
            move || Ok(universe.clone())
        }),
        Box::new(|_diff: &ChangedDiff| Vec::new()),
        Box::new(Vec::new),
        Box::new(|_universe: &[TestSelector]| Ok(CoverageFreshness::Fresh)),
        Box::new(|_universe: &[TestSelector]| PopulationPlan::default()),
        Box::new(|| Ok(SelectionDecision::default())),
    );

    assert_eq!(backer.language(), Language::Rust);
    assert_eq!(backer.discover_test_universe().unwrap(), universe);
    assert_eq!(backer.freshness(&[]).unwrap(), CoverageFreshness::Fresh);
    assert!(
        backer
            .changed_tests(&ChangedDiff::new(Vec::new()))
            .is_empty()
    );
}

#[test]
fn coverage_backer_forwards_to_callbacks() {
    let fixture = forwarding_fixture();
    let diff = ChangedDiff::new(vec![source(Language::Python, "app.py")]);
    assert_eq!(fixture.backer.language(), Language::Python);
    assert_eq!(
        fixture.backer.discover_test_universe().unwrap(),
        fixture.universe
    );
    assert_eq!(
        fixture.backer.changed_tests(&diff),
        vec![fixture.changed_test]
    );
    assert!(fixture.backer.prior_failures().is_empty());
    assert_eq!(
        fixture.backer.freshness(&fixture.universe).unwrap(),
        CoverageFreshness::Fresh
    );
    assert_eq!(
        fixture.backer.population_plan(&fixture.universe).selectors,
        fixture.universe
    );
    assert_eq!(
        fixture.backer.select().unwrap(),
        SelectionDecision {
            selectors: vec![fixture.selected],
            complete: true
        }
    );
    assert!(fixture.changed_seen.get());
    assert!(fixture.freshness_seen.get());
    assert!(fixture.population_seen.get());
    assert!(fixture.select_seen.get());
}

#[test]
fn fresh_backer_selects_affected_tests() {
    let (backer, select_calls) =
        FakeBacker::fresh(Language::Rust, vec![selector(Language::Rust, "b")]);
    let backer = backer.into_backer();
    let plan = CoverageDecisionEngine::new(vec![backer])
        .plan(&[source(Language::Rust, "src/lib.rs")])
        .unwrap();
    assert_eq!(plan.selected, vec![selector(Language::Rust, "b")]);
    assert!(plan.population.is_empty());
    assert_eq!(select_calls.get(), 1);
}

#[test]
fn fresh_incomplete_selection_escalates_to_population() {
    let select_calls = Rc::new(Cell::new(0));
    let backer = FakeBacker {
        language: Language::Python,
        universe: vec![
            selector(Language::Python, "a"),
            selector(Language::Python, "b"),
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

    let plan = CoverageDecisionEngine::new(vec![backer.into_backer()])
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
    let backer = FakeBacker {
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
    let plan = CoverageDecisionEngine::new(vec![backer.into_backer()])
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
    let backer = FakeBacker {
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
    let plan = CoverageDecisionEngine::new(vec![backer.into_backer()])
        .plan(&[source(Language::Python, "src/app.py")])
        .unwrap();
    assert_eq!(plan.population, vec![selector(Language::Python, "a")]);
    assert!(plan.selected.is_empty());
}

#[test]
fn changed_test_selectors_and_population_selectors_are_deduped() {
    let select_calls = Rc::new(Cell::new(0));
    let backer = FakeBacker {
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
    let plan = CoverageDecisionEngine::new(vec![backer.into_backer()])
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
    let (rust_backer, _rust_calls) =
        FakeBacker::fresh(Language::Rust, vec![selector(Language::Rust, "same")]);
    let (python_backer, _python_calls) =
        FakeBacker::fresh(Language::Python, vec![selector(Language::Python, "same")]);
    let plan =
        CoverageDecisionEngine::new(vec![rust_backer.into_backer(), python_backer.into_backer()])
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
