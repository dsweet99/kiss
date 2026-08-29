use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::collect::{collect_python_nodeids, reset_python_collect_memo_for_tests};
use crate::test_runner::python_coverage_index::PYTHON_SELECTOR_DISCOVERY_VERSION;
use crate::test_runner::runners::{
    enumerate_tests_in_changed_files, enumerate_workspace_python_selectors,
};

#[test]
fn full_suite_collection_omits_collect_ignore_glob_paths() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let fixtures = tests.join("fixtures");
    fs::create_dir_all(fixtures.join("mv/python")).unwrap();
    fs::write(
        tmp.path().join("tests/conftest.py"),
        "collect_ignore_glob = [\"fixtures/**\"]\n",
    )
    .unwrap();
    fs::write(
        fixtures.join("mv/python/test_ignored.py"),
        "def test_ignored():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_kept.py"),
        "def test_kept():\n    assert True\n",
    )
    .unwrap();
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &[], &[]).unwrap();
    assert_eq!(selectors, vec!["tests/test_kept.py::test_kept".to_string()]);
}

#[test]
fn workspace_collection_respects_kiss_ignore_prefixes() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let ignored = tmp.path().join("ignored_tests");
    fs::create_dir_all(&tests).unwrap();
    fs::create_dir_all(&ignored).unwrap();
    fs::write(
        tests.join("test_kept.py"),
        "def test_kept():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        ignored.join("test_ignored.py"),
        "def test_ignored():\n    assert True\n",
    )
    .unwrap();
    let ignore = ["ignored".to_string()];
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &ignore, &[]).unwrap();
    assert_eq!(selectors, vec!["tests/test_kept.py::test_kept".to_string()]);
}

#[test]
fn workspace_collection_respects_test_section_config_ignore() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    let ignored = tmp.path().join("ignored_tests");
    fs::create_dir_all(&tests).unwrap();
    fs::create_dir_all(&ignored).unwrap();
    fs::write(
        tests.join("test_kept.py"),
        "def test_kept():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        ignored.join("test_ignored.py"),
        "def test_ignored():\n    assert True\n",
    )
    .unwrap();
    let cfg = kiss::TestSectionConfig {
        ignore: vec!["ignored_tests/".to_string()],
        ..Default::default()
    };
    let ignore = cfg.merged_ignore(&[]);
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &ignore, &[]).unwrap();
    assert_eq!(selectors, vec!["tests/test_kept.py::test_kept".to_string()]);
}

#[test]
fn path_subset_collection_returns_only_requested_file_nodeids() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    let test_a = tests.join("test_a.py");
    let test_b = tests.join("test_b.py");
    fs::write(&test_a, "def test_a():\n    assert True\n").unwrap();
    fs::write(&test_b, "def test_b():\n    assert True\n").unwrap();
    let selectors =
        collect_python_nodeids(tmp.path(), Some(std::slice::from_ref(&test_a)), &[]).unwrap();
    assert_eq!(selectors, vec!["tests/test_a.py::test_a".to_string()]);
    fs::write(tmp.path().join("pytest.ini"), "[pytest]\naddopts = -q\n").unwrap();
    let after_config =
        collect_python_nodeids(tmp.path(), Some(std::slice::from_ref(&test_a)), &[]).unwrap();
    assert_eq!(after_config, vec!["tests/test_a.py::test_a".to_string()]);
}

#[test]
fn collection_import_error_surfaces_actionable_planning_error() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    let bad = tests.join("test_bad.py");
    fs::write(
        &bad,
        "import definitely_missing_module\n\ndef test_bad():\n    pass\n",
    )
    .unwrap();
    let err =
        collect_python_nodeids(tmp.path(), Some(std::slice::from_ref(&bad)), &[]).unwrap_err();
    assert!(err.contains("pytest collection failed"));
    assert!(err.contains("definitely_missing_module") || err.contains("ModuleNotFoundError"));
}

#[test]
fn parametrized_tests_expand_to_distinct_nodeids() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_param.py"),
        "import pytest\n\n@pytest.mark.parametrize('value', [1, 2])\ndef test_values(value):\n    assert value > 0\n",
    )
    .unwrap();
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &[], &[]).unwrap();
    assert_eq!(
        selectors,
        vec![
            "tests/test_param.py::test_values[1]".to_string(),
            "tests/test_param.py::test_values[2]".to_string(),
        ]
    );
}

#[test]
fn class_based_tests_use_pytest_nodeid_form() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_class.py"),
        "class TestValues:\n    def test_one(self):\n        assert True\n",
    )
    .unwrap();
    let selectors = enumerate_workspace_python_selectors(tmp.path(), &[], &[]).unwrap();
    assert_eq!(
        selectors,
        vec!["tests/test_class.py::TestValues::test_one".to_string()]
    );
}

#[test]
fn collection_memoization_avoids_second_child_process() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_one.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let first = collect_python_nodeids(tmp.path(), None, &[]).unwrap();
    let second = collect_python_nodeids(tmp.path(), None, &[]).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, vec!["tests/test_one.py::test_ok".to_string()]);
}

#[test]
fn changed_file_enumeration_uses_pytest_nodeids() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    let changed = tests.join("test_changed.py");
    fs::write(&changed, "def test_changed():\n    assert True\n").unwrap();
    let got = enumerate_tests_in_changed_files(tmp.path(), std::slice::from_ref(&changed)).unwrap();
    assert_eq!(
        got.python_nodeids,
        std::collections::BTreeSet::from(["tests/test_changed.py::test_changed".to_string()])
    );
}

#[test]
fn collection_invalid_request_surfaces_planning_error() {
    reset_python_collect_memo_for_tests();
    let err = collect_python_nodeids(Path::new(""), None, &[]).unwrap_err();
    assert!(err.contains("invalid pytest collection request"));
}

#[test]
fn format_collect_error_maps_all_collector_failures() {
    use super::collect::format_collect_error_for_test;
    use kiss::rpytest_runner::PytestCollectError;

    assert!(
        format_collect_error_for_test(PytestCollectError::InvalidRequest("bad".into()))
            .contains("invalid pytest collection request")
    );
    assert!(
        format_collect_error_for_test(PytestCollectError::Spawn {
            program: "python".into(),
            message: "nope".into(),
        })
        .contains("failed to spawn")
    );
    assert!(
        format_collect_error_for_test(PytestCollectError::CollectionFailed {
            exit_code: Some(2),
            stderr: "stderr detail".into(),
            stdout: String::new(),
        })
        .contains("stderr detail")
    );
    assert!(
        format_collect_error_for_test(PytestCollectError::CollectionFailed {
            exit_code: Some(2),
            stderr: String::new(),
            stdout: "stdout detail".into(),
        })
        .contains("stdout detail")
    );
    assert!(
        format_collect_error_for_test(PytestCollectError::InvalidOutput("bad json".into()))
            .contains("invalid pytest collection output")
    );
    assert!(
        format_collect_error_for_test(PytestCollectError::NodeidNormalization {
            nodeid: "x".into(),
            message: "bad path".into(),
        })
        .contains("invalid pytest nodeid")
    );
}

#[test]
fn pytest_collector_public_api_covers_path_subset_and_wrapper() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    let test_file = tests.join("test_subset.py");
    fs::write(&test_file, "def test_subset():\n    assert True\n").unwrap();
    let collector = kiss::rpytest_runner::subprocess_pytest_collector();
    let outcome = collector
        .collect(kiss::rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: PathBuf::from(
                std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
            ),
            paths: vec![test_file],
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(
        outcome.nodeids,
        vec!["tests/test_subset.py::test_subset".to_string()]
    );
}

#[test]
fn collection_memo_key_separates_path_subsets() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    let test_a = tests.join("test_a.py");
    let test_b = tests.join("test_b.py");
    fs::write(&test_a, "def test_a():\n    assert True\n").unwrap();
    fs::write(&test_b, "def test_b():\n    assert True\n").unwrap();
    let only_a =
        collect_python_nodeids(tmp.path(), Some(std::slice::from_ref(&test_a)), &[]).unwrap();
    let only_b =
        collect_python_nodeids(tmp.path(), Some(std::slice::from_ref(&test_b)), &[]).unwrap();
    assert_eq!(only_a, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(only_b, vec!["tests/test_b.py::test_b".to_string()]);
}

#[test]
fn collect_pytest_nodeids_public_wrapper_is_used() {
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_wrap.py"),
        "def test_wrap():\n    assert True\n",
    )
    .unwrap();
    let outcome =
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
    assert!(
        outcome
            .nodeids
            .contains(&"tests/test_wrap.py::test_wrap".to_string())
    );
}

#[test]
fn kiss_repo_discovery_omits_ignored_fixtures() {
    reset_python_collect_memo_for_tests();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let selectors = enumerate_workspace_python_selectors(&repo, &[], &[]).unwrap();
    assert!(
        !selectors
            .iter()
            .any(|selector| selector.contains("fixtures/mv/python"))
    );
}

#[test]
fn empty_repo_collection_succeeds_with_pytest_exit_code_five() {
    reset_python_collect_memo_for_tests();
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("source.py"), "VALUE = 1\n").unwrap();
    let selectors = collect_python_nodeids(tmp.path(), None, &[]).unwrap();
    assert!(selectors.is_empty());
}

#[test]
fn selector_discovery_version_is_v2() {
    assert_eq!(
        PYTHON_SELECTOR_DISCOVERY_VERSION,
        "python-selector-discovery-v2"
    );
}
