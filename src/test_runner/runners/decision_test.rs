use super::*;

#[test]
fn selector_plan_default_has_no_work_or_engine_claim() {
    let plan = SelectorPlan::default();

    assert!(plan.py_selectors.is_empty());
    assert!(plan.rust_selectors.is_empty());
    assert!(plan.rust_source_paths.is_empty());
    assert!(plan.rust_changed_lines.is_empty());
    assert!(plan.rust_source_population_paths.is_empty());
    assert!(plan.python_prior_failure_selectors.is_empty());
    assert!(plan.rust_prior_failure_selectors.is_empty());
    assert!(!plan.coverage_decision_engine_used);
}

#[test]
fn engine_backers_empty_when_no_language_has_work() {
    let tmp = tempfile::TempDir::new().unwrap();
    let changed_tests = ChangedTestSelectors::default();
    let rust_changed_lines = BTreeMap::new();
    let input = EngineBackerInputs {
        repo_root: tmp.path(),
        py_source_paths: &[],
        rust_source_paths: &[],
        rust_changed_lines: &rust_changed_lines,
        rust_test_args: &[],
        lang_filter: None,
        ignore: &[],
        changed_tests: &changed_tests,
    };

    assert!(engine_backers(input).unwrap().backers.is_empty());
}

#[test]
fn python_source_change_populates_full_test_universe() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    std::fs::create_dir(&tests).unwrap();
    let app = tmp.path().join("app.py");
    let test_app = tests.join("test_app.py");
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    std::fs::write(
        &test_app,
        "def test_one():\n    assert True\n\ndef test_two():\n    assert True\n",
    )
    .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&app),
        &[],
        &BTreeMap::new(),
        &[],
        Some(kiss::Language::Python),
        &[],
    )
    .unwrap();

    assert_eq!(
        plan.py_selectors,
        vec![
            py_selector(&test_app, "test_one"),
            py_selector(&test_app, "test_two")
        ]
    );
    assert!(plan.rust_selectors.is_empty());
}
