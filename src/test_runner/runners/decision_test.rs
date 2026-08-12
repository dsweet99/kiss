use super::*;
use crate::test_runner::coverage_decision::{LanguagePlanner, SelectionDecision};
use crate::test_runner::python_coverage_index::{
    python_coverage_cache_root, rebuild_python_coverage_index,
};
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args,
};
use rpytest_runner::TestStatus;
use rslip::LineCoverage;
use std::time::Duration;

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

    assert!(
        prior_failures_for_language(tmp.path(), kiss::Language::Python, &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        prior_failures_for_language(tmp.path(), kiss::Language::Rust, &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn prior_failures_for_language_loads_rust_failures_with_live_identity() {
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

    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    let cargo_version =
        crate::test_runner::runners::command_stdout(&cargo, &["--version"], tmp.path())
            .expect("cargo version");
    let llvm_cov_version =
        crate::test_runner::runners::command_stdout(&cargo, &["llvm-cov", "--version"], tmp.path())
            .expect("llvm-cov version");
    let cargo_nextest_version =
        crate::test_runner::runners::command_stdout(&cargo, &["nextest", "--version"], tmp.path())
            .expect("nextest version");
    let rustc_version = crate::test_runner::runners::command_stdout(&rustc, &["-Vv"], tmp.path())
        .expect("rustc version");
    let runner_map_fingerprint =
        crate::test_runner::rust_coverage_index::current_rust_runner_map_fingerprint(
            tmp.path(),
            &[],
        )
        .unwrap_or_default();
    let identity = crate::test_runner::last_status::rust_last_status_identity(
        &cargo_version,
        &llvm_cov_version,
        &rustc_version,
        &cargo_nextest_version,
        &[],
        &runner_map_fingerprint,
    );
    crate::test_runner::last_status::record_statuses(
        tmp.path(),
        kiss::Language::Rust,
        &identity,
        &[(
            "demo::tests::failed".to_string(),
            rpytest_runner::TestStatus::Failed,
        )],
    )
    .unwrap();

    let loaded = prior_failures_for_language(tmp.path(), kiss::Language::Rust, &[]).unwrap();
    assert_eq!(
        loaded,
        vec![TestSelector::new(
            kiss::Language::Rust,
            "demo::tests::failed"
        )]
    );
}

#[test]
fn prior_failures_for_language_loads_python_failures_with_live_identity() {
    let tmp = tempfile::TempDir::new().unwrap();
    let python = PathBuf::from("python");
    let python_version = crate::test_runner::runners::command_stdout(
        &python,
        &[
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
        tmp.path(),
    )
    .expect("python version");
    let pytest_version = crate::test_runner::runners::command_stdout(
        &python,
        &["-c", "import pytest; print(pytest.__version__)"],
        tmp.path(),
    )
    .expect("pytest version");
    let identity = crate::test_runner::last_status::python_last_status_identity(
        &python_version,
        &pytest_version,
        &[],
    );
    crate::test_runner::last_status::record_statuses(
        tmp.path(),
        kiss::Language::Python,
        &identity,
        &[(
            "tests/test_app.py::test_failed".to_string(),
            rpytest_runner::TestStatus::Failed,
        )],
    )
    .unwrap();

    let loaded = prior_failures_for_language(tmp.path(), kiss::Language::Python, &[]).unwrap();
    assert_eq!(
        loaded,
        vec![TestSelector::new(
            kiss::Language::Python,
            "tests/test_app.py::test_failed"
        )]
    );
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
        plan.selectors.python
            .iter()
            .any(|selector| selector.ends_with("test_app.py::test_py_changed"))
    );
    assert!(
        plan.selectors.rust
            .iter()
            .any(|selector| selector.contains("rust_changed"))
    );
    assert!(plan.coverage_decision_engine_used);
    assert_eq!(plan.vcs_source_paths.rust, 0);
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
