use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rpytest_runner::{PytestRunRequest, SubprocessPytestRunner, TestStatus};

fn python() -> PathBuf {
    match std::env::var_os("PYTHON") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("python"),
    }
}

fn request(root: &std::path::Path, nodeid: &str) -> PytestRunRequest {
    PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: root.to_path_buf(),
        python: python(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        child_preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    }
}

#[test]
fn subprocess_pytest_runner_reports_pass_and_fail_statuses() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n\ndef test_fail():\n    assert False\n",
    )
    .unwrap();
    let runner = SubprocessPytestRunner::new();

    let passed = runner
        .run_one(request(tmp.path(), "test_sample.py::test_ok"))
        .unwrap();
    let failed = runner
        .run_one(request(tmp.path(), "test_sample.py::test_fail"))
        .unwrap();

    assert_eq!(passed.status, TestStatus::Passed);
    assert_eq!(failed.status, TestStatus::Failed);
    assert_eq!(failed.exit_code, Some(1));
}

#[test]
fn subprocess_pytest_runner_run_many_keeps_request_order() {
    let ok_tmp = tempfile::tempdir().unwrap();
    let fail_tmp = tempfile::tempdir().unwrap();
    fs::write(
        ok_tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        fail_tmp.path().join("test_sample.py"),
        "def test_fail():\n    assert False\n",
    )
    .unwrap();

    let outcomes = SubprocessPytestRunner::new().run_many(vec![
        request(ok_tmp.path(), "test_sample.py::test_ok"),
        request(fail_tmp.path(), "test_sample.py::test_fail"),
    ]);

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Failed);
}
