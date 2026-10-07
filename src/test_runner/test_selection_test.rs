use super::{
    ChangedDiff, ChangedSource, LanguagePlanner, SelectionBasis, SelectionEngine, TestSelector,
};
use kiss::Language;
use std::cell::Cell;
use std::rc::Rc;

struct FakePlanner {
    language: Language,
    universe: Vec<TestSelector>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
    sources_changed: bool,
    universe_calls: Rc<Cell<usize>>,
}

impl FakePlanner {
    fn new(language: Language, sources_changed: bool) -> (Self, Rc<Cell<usize>>) {
        let universe_calls = Rc::new(Cell::new(0));
        (
            Self {
                language,
                universe: vec![selector(language, "a"), selector(language, "b")],
                changed_tests: vec![],
                prior_failures: vec![],
                sources_changed,
                universe_calls: Rc::clone(&universe_calls),
            },
            universe_calls,
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
        self.universe_calls.set(self.universe_calls.get() + 1);
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

    fn sources_changed(&self) -> bool {
        self.sources_changed
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
    let (planner, _) = FakePlanner::new(Language::Rust, false);
    assert_eq!(planner.language(), Language::Rust);
    assert_eq!(planner.selection_basis(), SelectionBasis::Current);
    assert!(
        planner
            .changed_tests(&ChangedDiff::new(Vec::new()))
            .is_empty()
    );
    let (changed, _) = FakePlanner::new(Language::Rust, true);
    assert_eq!(changed.selection_basis(), SelectionBasis::Population);
}

#[test]
fn test_selector_ordering_groups_python_before_rust() {
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
}

#[test]
fn unchanged_sources_select_only_changed_tests() {
    let (mut planner, universe_calls) = FakePlanner::new(Language::Rust, false);
    planner.changed_tests = vec![selector(Language::Rust, "b")];
    let plan = SelectionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib_test.rs")])
        .unwrap();
    assert_eq!(plan.selected, vec![selector(Language::Rust, "b")]);
    assert!(plan.population.is_empty());
    assert!(plan.population_languages.is_empty());
    assert_eq!(universe_calls.get(), 0);
}

#[test]
fn prior_failures_outside_pytest_universe_are_dropped() {
    let (mut planner, _) = FakePlanner::new(Language::Python, false);
    planner.prior_failures = vec![
        selector(Language::Python, "a"),
        selector(
            Language::Python,
            "/abs/tests/fixtures/mv/python/test.py::test_stale",
        ),
    ];
    let plan = SelectionEngine::new(vec![planner.boxed()])
        .plan(&[])
        .unwrap();
    assert_eq!(plan.selected, vec![selector(Language::Python, "a")]);
}

#[test]
fn changed_source_runs_population_with_changed_tests_and_prior_failures() {
    let (mut planner, _) = FakePlanner::new(Language::Python, true);
    planner.changed_tests = vec![selector(Language::Python, "changed")];
    planner.prior_failures = vec![
        selector(Language::Python, "a"),
        selector(Language::Python, "gone"),
    ];
    let plan = SelectionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Python, "src/app.py")])
        .unwrap();
    assert_eq!(
        plan.population,
        vec![
            selector(Language::Python, "a"),
            selector(Language::Python, "b"),
            selector(Language::Python, "changed"),
        ]
    );
    assert_eq!(plan.population_languages, vec![Language::Python]);
    assert!(plan.selected.is_empty());
}

#[test]
fn changed_test_selectors_and_population_selectors_are_deduped() {
    let (mut planner, _) = FakePlanner::new(Language::Rust, true);
    planner.changed_tests = vec![selector(Language::Rust, "a"), selector(Language::Rust, "a")];
    let plan = SelectionEngine::new(vec![planner.boxed()])
        .plan(&[source(Language::Rust, "src/lib.rs")])
        .unwrap();
    assert_eq!(
        plan.population,
        vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")]
    );
    assert!(plan.selected.is_empty());
}

#[test]
fn multiple_language_backers_combine_without_selector_collisions() {
    let (mut rust_planner, _) = FakePlanner::new(Language::Rust, false);
    rust_planner.changed_tests = vec![selector(Language::Rust, "same")];
    let (mut python_planner, _) = FakePlanner::new(Language::Python, false);
    python_planner.changed_tests = vec![selector(Language::Python, "same")];
    let plan = SelectionEngine::new(vec![rust_planner.boxed(), python_planner.boxed()])
        .plan(&[
            source(Language::Rust, "src/lib_test.rs"),
            source(Language::Python, "tests/test_app.py"),
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
fn one_language_population_leaves_the_other_selective() {
    let (rust_planner, _) = FakePlanner::new(Language::Rust, true);
    let (mut python_planner, _) = FakePlanner::new(Language::Python, false);
    python_planner.prior_failures = vec![selector(Language::Python, "a")];
    let plan = SelectionEngine::new(vec![rust_planner.boxed(), python_planner.boxed()])
        .plan(&[
            source(Language::Rust, "src/lib.rs"),
            source(Language::Python, "tests/test_app.py"),
        ])
        .unwrap();
    assert_eq!(
        plan.population,
        vec![selector(Language::Rust, "a"), selector(Language::Rust, "b")]
    );
    assert_eq!(plan.population_languages, vec![Language::Rust]);
    assert_eq!(plan.selected, vec![selector(Language::Python, "a")]);
}

#[test]
fn supported_language_unifies_planner_and_runtime_stacks() {
    use crate::test_runner::lang_iface::LanguageRuntime;
    use crate::test_runner::lang_python::PythonRuntime;
    use crate::test_runner::lang_rust::RustRuntime;
    use crate::test_runner::test_selection::SupportedLanguage;

    let python = PythonRuntime::default();
    assert_eq!(
        <PythonRuntime as SupportedLanguage>::language(&python),
        Language::Python
    );
    assert_eq!(
        <RustRuntime as SupportedLanguage>::language(&RustRuntime::default()),
        Language::Rust
    );
    let runtimes: [&dyn LanguageRuntime; 2] = [&python, &RustRuntime::default()];
    assert_eq!(
        runtimes.map(|runtime| runtime.language()),
        [Language::Python, Language::Rust]
    );
}
