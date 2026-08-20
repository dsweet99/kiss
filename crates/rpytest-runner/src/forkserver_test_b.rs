use std::collections::BTreeMap;
use std::fs;

use crate::forkserver::run_with_reused_controller;
use crate::forkserver_test_support::{base_req, test_python};
use crate::{ForkserverPytestRunner, PytestRunRequest, TestStatus};

#[test]
fn forkserver_run_with_reused_controller_keeps_controller_alive() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut controller = None;

    let outcome = run_with_reused_controller(
        &mut controller,
        base_req(tmp.path(), "test_sample.py::test_ok"),
    )
    .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert!(controller.is_some());
}

#[test]
fn forkserver_imports_preload_after_child_env_setup() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("preload_flag.py"),
        "import os\nopen(os.environ['FLAG_PATH'], 'w').write('loaded')\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let flag_path = tmp.path().join("flag.txt");
    let mut env = BTreeMap::new();
    env.insert(
        "PYTHONPATH".to_string(),
        tmp.path().to_string_lossy().to_string(),
    );
    env.insert(
        "FLAG_PATH".to_string(),
        flag_path.to_string_lossy().to_string(),
    );

    let outcome = ForkserverPytestRunner::new()
        .run_one(PytestRunRequest::from_parts(
            "test_sample.py::test_ok".to_string(),
            tmp.path().to_path_buf(),
            test_python(),
            vec!["-q".to_string()],
            env,
            vec!["preload_flag".to_string()],
            Vec::new(),
            None,
        ))
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(fs::read_to_string(flag_path).unwrap(), "loaded");
}
