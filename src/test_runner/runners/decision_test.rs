use super::*;
use crate::test_runner::coverage_decision::{ChangedDiff, SelectionDecision};
use crate::test_runner::python_coverage_index::{
    python_coverage_cache_root, rebuild_python_coverage_index,
    write_python_population_manifest_for_args,
};
use crate::test_runner::rust_coverage_index::{
    CACHE_SCHEMA_VERSION, rebuild_rust_coverage_index, rust_coverage_cache_root,
};
use rpytest_runner::TestStatus;
use rslip::LineCoverage;
use std::time::Duration;

#[path = "decision_policy_test.rs"]
mod policy_tests;

#[test]
fn selector_plan_default_has_no_work_or_engine_claim() {
    let plan = SelectorPlan::default();

    assert!(plan.py_selectors.is_empty());
    assert!(plan.rust_selectors.is_empty());
    assert!(!plan.python_population_required);
    assert!(plan.python_population_selectors.is_empty());
    assert!(plan.rust_population_selectors.is_empty());
    assert!(plan.rust_source_paths.is_empty());
    assert!(plan.python_changed_lines.is_empty());
    assert!(plan.rust_changed_lines.is_empty());
    assert!(plan.rust_source_population_paths.is_empty());
    assert!(plan.python_prior_failure_selectors.is_empty());
    assert!(plan.rust_prior_failure_selectors.is_empty());
    assert!(!plan.coverage_decision_engine_used);
}

#[test]
#[allow(non_snake_case)]
fn EngineBackers_empty_when_no_language_has_work() {
    // Covers EngineBackers empty planner/prior-failure output.
    let tmp = tempfile::TempDir::new().unwrap();
    let changed_tests = ChangedTestSelectors::default();
    let python_changed_lines = BTreeMap::new();
    let rust_changed_lines = BTreeMap::new();
    let input = EngineBackerInputs {
        repo_root: tmp.path(),
        py_source_paths: &[],
        python_changed_lines: &python_changed_lines,
        rust_source_paths: &[],
        rust_changed_lines: &rust_changed_lines,
        rust_test_args: &[],
        lang_filter: None,
        ignore: &[],
        changed_tests: &changed_tests,
    };

    let backers = engine_backers(input).unwrap();
    assert!(backers.backers.is_empty());
    assert!(backers.prior_failures.is_empty());
}

#[test]
fn engine_backers_expose_manifest_env_policy() {
    let tmp = tempfile::TempDir::new().unwrap();
    let app = tmp.path().join("app.py");
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::write(&app, "VALUE = 1\n").unwrap();
    std::fs::write(&lib, "pub fn value() -> i32 { 1 }\n").unwrap();
    let changed_tests = ChangedTestSelectors::default();
    let python_changed_lines = BTreeMap::new();
    let rust_changed_lines = BTreeMap::new();
    let input = EngineBackerInputs {
        repo_root: tmp.path(),
        py_source_paths: std::slice::from_ref(&app),
        python_changed_lines: &python_changed_lines,
        rust_source_paths: std::slice::from_ref(&lib),
        rust_changed_lines: &rust_changed_lines,
        rust_test_args: &[],
        lang_filter: None,
        ignore: &[],
        changed_tests: &changed_tests,
    };

    let engine_backers = engine_backers(input).unwrap();
    assert!(engine_backers.prior_failures.is_empty());
    let backers = engine_backers.backers;
    let python = backers
        .iter()
        .find(|backer| backer.language() == kiss::Language::Python)
        .unwrap();
    let rust = backers
        .iter()
        .find(|backer| backer.language() == kiss::Language::Rust)
        .unwrap();

    assert_eq!(python.manifest_env_allowlist(), ["PYTHONPATH"]);
    assert!(rust.manifest_env_allowlist().contains(&"RUSTFLAGS"));
}

#[test]
#[allow(non_snake_case)]
fn PythonModule_and_RustModule_expose_discovery_and_static_policy_inputs() {
    // Covers PythonModule and RustModule planner methods.
    let tmp = tempfile::TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    let test_app = tests.join("test_app.py");
    let lib = src.join("lib.rs");
    std::fs::write(&test_app, "def test_value():\n    assert True\n").unwrap();
    std::fs::write(&lib, "#[test]\nfn rust_value() {}\n").unwrap();
    let py_changed =
        TestSelector::new(kiss::Language::Python, py_selector(&test_app, "test_value"));
    let rs_changed = TestSelector::new(kiss::Language::Rust, "rust_value");
    let py_prior = TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_prior");
    let rs_prior = TestSelector::new(kiss::Language::Rust, "crate::tests::test_prior");

    let python = python_backer::PythonModule::new(
        tmp.path(),
        &[],
        &BTreeMap::new(),
        &[],
        &[],
        std::slice::from_ref(&py_changed),
        std::slice::from_ref(&py_prior),
    );
    let rust = RustModule::new(
        tmp.path(),
        &[],
        &BTreeMap::new(),
        &[],
        &[],
        std::slice::from_ref(&rs_changed),
        std::slice::from_ref(&rs_prior),
    );
    let diff = ChangedDiff::new(vec![
        ChangedSource::new(kiss::Language::Python, "app.py"),
        ChangedSource::new(kiss::Language::Rust, "src/lib.rs"),
    ]);

    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::language(&python),
        kiss::Language::Python
    );
    assert!(
        <python_backer::PythonModule as LanguagePlanner>::discover_universe(&python)
            .unwrap()
            .contains(&py_changed)
    );
    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::changed_tests(&python, &diff),
        vec![py_changed]
    );
    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::prior_failures(&python),
        vec![py_prior]
    );
    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::manifest_env_allowlist(&python),
        ["PYTHONPATH"]
    );

    assert_eq!(
        <RustModule as LanguagePlanner>::language(&rust),
        kiss::Language::Rust
    );
    assert!(
        <RustModule as LanguagePlanner>::discover_universe(&rust)
            .unwrap()
            .contains(&rs_changed)
    );
    assert_eq!(
        <RustModule as LanguagePlanner>::changed_tests(&rust, &diff),
        vec![rs_changed]
    );
    assert_eq!(
        <RustModule as LanguagePlanner>::prior_failures(&rust),
        vec![rs_prior]
    );
    assert!(<RustModule as LanguagePlanner>::manifest_env_allowlist(&rust).contains(&"RUSTFLAGS"));
}

#[test]
fn select_fresh_python_source_selectors_and_select_fresh_rust_source_selectors_changed_line_coverage()
 {
    // Covers select_fresh_python_source_selectors and select_fresh_rust_source_selectors.
    let tmp = tempfile::TempDir::new().unwrap();
    let app = tmp.path().join("app.py");
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    std::fs::write(&lib, "pub fn value() -> i32 { 1 }\n").unwrap();
    write_python_entry(
        tmp.path(),
        "py",
        "tests/test_app.py::test_value",
        LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_rust_entry(
        tmp.path(),
        "rs",
        "crate::tests::test_value",
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    rebuild_python_coverage_index(tmp.path()).unwrap();
    rebuild_rust_coverage_index(tmp.path()).unwrap();

    assert_eq!(
        python_backer::select_fresh_python_source_selectors(
            tmp.path(),
            std::slice::from_ref(&app),
            &BTreeMap::from([(app.clone(), BTreeSet::from([1]))]),
        ),
        Some(BTreeSet::from(
            ["tests/test_app.py::test_value".to_string()]
        ))
    );
    assert_eq!(
        select_fresh_rust_source_selectors(
            tmp.path(),
            std::slice::from_ref(&lib),
            &BTreeMap::from([(lib.clone(), BTreeSet::from([1]))]),
        ),
        Some(BTreeSet::from(["crate::tests::test_value".to_string()]))
    );

    let python = python_backer::PythonModule::new(
        tmp.path(),
        std::slice::from_ref(&app),
        &BTreeMap::from([(app.clone(), BTreeSet::from([1]))]),
        &[],
        &[],
        &[],
        &[],
    );
    let rust = RustModule::new(
        tmp.path(),
        std::slice::from_ref(&lib),
        &BTreeMap::from([(lib.clone(), BTreeSet::from([1]))]),
        &[],
        &[],
        &[],
        &[],
    );

    assert_eq!(
        <python_backer::PythonModule as LanguagePlanner>::select(&python).unwrap(),
        SelectionDecision {
            selectors: vec![TestSelector::new(
                kiss::Language::Python,
                "tests/test_app.py::test_value"
            )],
            complete: true,
        }
    );
    assert_eq!(
        <RustModule as LanguagePlanner>::select(&rust).unwrap(),
        SelectionDecision {
            selectors: vec![TestSelector::new(
                kiss::Language::Rust,
                "crate::tests::test_value"
            )],
            complete: true,
        }
    );
}

#[test]
fn python_source_change_requires_population_without_selective_python() {
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

    assert!(plan.py_selectors.is_empty());
    assert!(plan.python_population_required);
    assert_eq!(
        plan.python_population_selectors,
        vec![
            py_selector(&test_app, "test_one"),
            py_selector(&test_app, "test_two")
        ]
    );
    assert!(plan.rust_selectors.is_empty());
}

#[test]
fn warm_python_source_change_selects_covering_test_from_rslip_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    std::fs::create_dir(&tests).unwrap();
    let app = tmp.path().join("app.py");
    let test_app = tests.join("test_app.py");
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    std::fs::write(
        &test_app,
        "from app import value\n\n\
def test_value():\n    assert value() == 1\n\n\
def test_unrelated():\n    assert True\n",
    )
    .unwrap();
    let covering = py_selector(&test_app, "test_value");
    let unrelated = py_selector(&test_app, "test_unrelated");
    write_python_entry(
        tmp.path(),
        "covering",
        &covering,
        LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_python_entry(
        tmp.path(),
        "unrelated",
        &unrelated,
        LineCoverage {
            files: BTreeMap::from([(test_app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    rebuild_python_coverage_index(tmp.path()).unwrap();
    write_python_population_manifest_for_args(
        tmp.path(),
        &[covering.clone(), unrelated.clone()],
        &[],
    )
    .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        std::slice::from_ref(&app),
        &[],
        &BTreeMap::from([(app.clone(), BTreeSet::from([1]))]),
        &[],
        Some(kiss::Language::Python),
        &[],
    )
    .unwrap();

    assert_eq!(plan.py_selectors, vec![covering]);
    assert!(!plan.python_population_required);
    assert!(plan.python_population_selectors.is_empty());
}

fn write_python_entry(
    repo_root: &std::path::Path,
    name: &str,
    selector: &str,
    coverage: LineCoverage,
) {
    let path = python_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": rslip::CACHE_SCHEMA_VERSION,
        "nodeid": selector,
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    std::fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

fn write_rust_entry(
    repo_root: &std::path::Path,
    name: &str,
    selector: &str,
    coverage: rust_llvm_cov_runner::RustLineCoverage,
) {
    let path = rust_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": CACHE_SCHEMA_VERSION,
        "selector": selector,
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    std::fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}
