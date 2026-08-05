use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::python_collect::reset_python_collect_memo_for_tests;
use crate::test_runner::coverage_decision::LanguageExecutor;
use crate::test_runner::runners::{
    enumerate_workspace_python_selectors, python_backer::PythonModule,
};

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
        rpytest_runner::collect_pytest_nodeids(rpytest_runner::PytestCollectRequest {
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
