use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::collect::reset_python_collect_memo_for_tests;
use crate::test_runner::runners::{
    enumerate_workspace_python_selectors, python_backer::PythonModule,
};
use crate::test_runner::test_selection::{LanguageExecutor, LanguagePlanner};

#[test]
fn kiss_discovery_matches_isolated_pytest_collection() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let fixtures = tests.join("fixtures");
    fs::create_dir_all(&fixtures).unwrap();
    fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\ntestpaths = tests\npython_files = test_*.py\n",
    )
    .unwrap();
    fs::write(
        tests.join("conftest.py"),
        "collect_ignore_glob = [\"fixtures/**\"]\n",
    )
    .unwrap();
    fs::write(
        fixtures.join("test_ignored.py"),
        "def test_ignored():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_param.py"),
        "import pytest\n\n@pytest.mark.parametrize('value', [1, 2])\ndef test_values(value):\n    assert value > 0\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_class.py"),
        "class TestValues:\n    def test_one(self):\n        assert True\n",
    )
    .unwrap();
    let kiss_selectors = enumerate_workspace_python_selectors(tmp.path(), &[], &[]).unwrap();
    let pytest_outcome =
        kiss::rpytest_runner::collect_pytest_nodeids(kiss::rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: PathBuf::from(
                std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
            ),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(kiss_selectors, pytest_outcome.nodeids);
    assert_eq!(
        kiss_selectors,
        vec![
            "tests/test_class.py::TestValues::test_one".to_string(),
            "tests/test_param.py::test_values[1]".to_string(),
            "tests/test_param.py::test_values[2]".to_string(),
        ]
    );
}

#[test]
fn flat_repo_python_files_pattern_is_collected() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\npython_files = check_*.py\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("check_bad.py"),
        "def test_bad():\n    assert False\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_hidden.py"),
        "def test_hidden():\n    assert False\n",
    )
    .unwrap();
    let ignore = vec!["fixtures".to_string()];
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &ignore, &[]).unwrap();
    assert_eq!(selectors, vec!["check_bad.py::test_bad".to_string()]);
}

#[test]
fn dry_run_lines_omit_ignored_fixture_selectors() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let fixtures = tests.join("fixtures").join("mv").join("python");
    fs::create_dir_all(&fixtures).unwrap();
    fs::write(
        tests.join("conftest.py"),
        "collect_ignore_glob = [\"fixtures/**\"]\n",
    )
    .unwrap();
    fs::write(
        fixtures.join("test_ignored.py"),
        "def test_ignored():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_kept.py"),
        "def test_kept():\n    assert True\n",
    )
    .unwrap();
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &[], &[]).unwrap();
    let module = PythonModule::for_execution(tmp.path(), &[]);
    let lines = module.dry_run_lines(&selectors, false, &[], 1).unwrap();
    let output = lines.join("\n");
    assert!(!output.contains("fixtures/mv/python"));
    assert!(output.contains("tests/test_kept.py::test_kept"));
}

#[test]
fn discovery_with_pytest_args_collects_current_tests() {
    let _lock = crate::cwd_test_lock::lock();
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(
        tmp.path().join("tests/test_extra.py"),
        "def test_extra():\n    assert True\n",
    )
    .unwrap();
    let pytest_args = vec!["-p".to_string(), "pytest_asyncio.plugin".to_string()];

    let enumerated = enumerate_workspace_python_selectors(tmp.path(), &[], &pytest_args).unwrap();
    let expected = vec!["tests/test_extra.py::test_extra".to_string()];
    assert_eq!(enumerated, expected);

    let module = PythonModule::for_execution_with_args(tmp.path(), &[], &pytest_args);
    let discovered = module
        .discover_universe()
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(discovered, expected);
}
