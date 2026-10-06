use super::*;
use crate::test_runner::coverage_decision::{LanguagePlanner, SelectionDecision};
use crate::test_runner::rust_coverage_index::rebuild_rust_coverage_index;
use kiss::rpytest_runner::TestStatus;
use kiss::rslip::LineCoverage;

#[path = "decision_line_coverage_test.rs"]
mod line_coverage_tests;
#[path = "decision_policy_test.rs"]
mod policy_tests;

#[test]
fn selector_plan_default_has_no_work_or_engine_claim() {
    let plan = SelectorPlan::default();

    assert!(plan.selectors.python.is_empty());
    assert!(plan.selectors.rust.is_empty());
    assert!(!plan.population_required.python);
    assert!(!plan.population_required.rust);
    assert!(plan.source_paths.rust.is_empty());
    assert!(plan.changed_lines.python.is_empty());
    assert!(plan.changed_lines.rust.is_empty());
    assert!(plan.prior_failure_selectors.python.is_empty());
    assert!(plan.prior_failure_selectors.rust.is_empty());
    assert!(!plan.coverage_decision_engine_used);
}

#[test]
fn decision_helper_splitters_preserve_language_and_line_filters() {
    let py = PathBuf::from("app.py");
    let rs = PathBuf::from("src/lib.rs");
    let (py_sources, rust_sources) = split_source_paths(&[py.clone(), rs.clone()]);
    assert_eq!(py_sources, vec![py.clone()]);
    assert_eq!(rust_sources, vec![rs.clone()]);

    let changed = changed_sources_for_engine(&py_sources, &rust_sources);
    assert_eq!(changed.len(), 2);
    assert!(
        changed
            .iter()
            .any(|source| source.language == kiss::Language::Python)
    );
    assert!(
        changed
            .iter()
            .any(|source| source.language == kiss::Language::Rust)
    );

    let lines = BTreeMap::from([
        (py.clone(), BTreeSet::from([1, 2])),
        (rs.clone(), BTreeSet::from([3])),
    ]);
    assert_eq!(
        changed_lines_for_sources(&lines, std::slice::from_ref(&rs)),
        BTreeMap::from([(rs.clone(), BTreeSet::from([3]))])
    );

    let selectors = vec![
        TestSelector::new(kiss::Language::Rust, "rs::test"),
        TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_app"),
        TestSelector::new(kiss::Language::Rust, "rs::test"),
    ];
    let (py_sel, rs_sel) = selectors_by_language(&selectors);
    assert_eq!(py_sel, vec!["tests/test_app.py::test_app".to_string()]);
    assert_eq!(rs_sel, vec!["rs::test".to_string()]);
}

#[test]
fn prior_failure_and_basis_helpers_have_empty_cases() {
    let tmp = tempfile::TempDir::new().unwrap();

    assert!(prior_failures_for_language(tmp.path(), kiss::Language::Python).is_empty());
    assert!(prior_failures_for_language(tmp.path(), kiss::Language::Rust).is_empty());
}

fn seed_record(root: &std::path::Path, language: kiss::Language, id: &str, status: TestStatus) {
    let dir = kiss::test_records::records_dir(root, language.label());
    std::fs::create_dir_all(&dir).unwrap();
    kiss::test_records::store_record(
        &dir,
        &kiss::test_records::TestRecord {
            schema: kiss::test_records::RECORD_SCHEMA.to_string(),
            language: language.label().to_string(),
            test_id: id.to_string(),
            identity: "identity".to_string(),
            deps: BTreeMap::new(),
            status,
            exit_code: None,
            duration: std::time::Duration::ZERO,
            covered: BTreeMap::new(),
        },
    )
    .unwrap();
}

#[test]
fn prior_failures_for_language_reads_nonpassed_rust_records() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
    seed_record(
        tmp.path(),
        kiss::Language::Rust,
        "demo::tests::failed",
        TestStatus::Failed,
    );
    seed_record(
        tmp.path(),
        kiss::Language::Rust,
        "demo::tests::passed",
        TestStatus::Passed,
    );

    assert_eq!(
        prior_failures_for_language(tmp.path(), kiss::Language::Rust),
        vec![TestSelector::new(
            kiss::Language::Rust,
            "demo::tests::failed"
        )]
    );
    assert!(
        super::current_prior_failures(tmp.path(), kiss::Language::Rust, &[], &[])
            .unwrap()
            .is_empty(),
        "removed tests must not return through prior-failure selection"
    );
}

#[test]
fn prior_failures_for_language_reads_nonpassed_python_records() {
    let tmp = tempfile::TempDir::new().unwrap();
    seed_record(
        tmp.path(),
        kiss::Language::Python,
        "tests/test_app.py::test_failed",
        TestStatus::Failed,
    );
    seed_record(
        tmp.path(),
        kiss::Language::Python,
        "tests/test_app.py::test_slow",
        TestStatus::TimedOut,
    );
    seed_record(
        tmp.path(),
        kiss::Language::Python,
        "tests/test_app.py::test_ok",
        TestStatus::Passed,
    );

    assert_eq!(
        prior_failures_for_language(tmp.path(), kiss::Language::Python),
        vec![
            TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_failed"),
            TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_slow"),
        ]
    );
    assert!(prior_failures_for_language(tmp.path(), kiss::Language::Rust).is_empty());
}

#[test]
fn combined_selectors_routes_changed_python_and_rust_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let py_test = tests.join("test_app.py");
    let rs_test = src.join("lib.rs");
    std::fs::write(&py_test, "def test_py_changed():\n    assert True\n").unwrap();
    std::fs::write(
        &rs_test,
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn rust_changed() { assert_eq!(1, 1); }\n}\n",
    )
    .unwrap();
    // Seeded universe avoids pytest collect in helper/universe expansion.
    crate::test_runner::python_coverage_index::write_python_population_manifest_for_args(
        tmp.path(),
        &["tests/test_app.py::test_py_changed".to_string()],
        &[],
    )
    .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        &[],
        &[py_test.clone(), rs_test],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert!(
        plan.selectors
            .python
            .iter()
            .any(|selector| selector.ends_with("test_app.py::test_py_changed"))
    );
    assert!(
        plan.selectors
            .rust
            .iter()
            .any(|selector| selector.contains("rust_changed"))
    );
    assert!(plan.coverage_decision_engine_used);
    assert_eq!(plan.vcs_source_paths.rust, 0);
}

#[test]
fn changed_python_helper_without_selector_selects_language_universe() {
    crate::test_runner::lang_python::collect::reset_python_collect_memo_for_tests();
    let tmp = tempfile::TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(
        tests.join("test_app.py"),
        "def test_app():\n    assert True\n",
    )
    .unwrap();
    let helper = tests.join("helpers.py");
    std::fs::write(&helper, "def helper():\n    return 1\n").unwrap();
    crate::test_runner::python_coverage_index::write_python_population_manifest_for_args(
        tmp.path(),
        &["tests/test_app.py::test_app".to_string()],
        &[],
    )
    .unwrap();

    let plan = combined_selectors(
        tmp.path(),
        &[],
        std::slice::from_ref(&helper),
        &BTreeMap::new(),
        &[],
        Some(kiss::Language::Python),
        &[],
    )
    .unwrap();

    assert!(
        plan.selectors
            .python
            .iter()
            .any(|selector| selector.ends_with("test_app.py::test_app")),
        "unresolved Python helper must fall back to the language test universe, got {:?}",
        plan.selectors.python
    );
}

#[test]
#[allow(non_snake_case)]
fn EngineBackers_empty_when_no_language_has_work() {
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
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        lang_filter: None,
        ignore: &[],
        changed_tests: &changed_tests,
        rust_resolved: None,
        include_prior_failures: true,
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
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        lang_filter: None,
        ignore: &[],
        changed_tests: &changed_tests,
        rust_resolved: None,
        include_prior_failures: true,
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
