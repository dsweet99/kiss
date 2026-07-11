use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::runner::{run_command, spawn_subprocess_job, validate_request};
use crate::{PytestRunError, PytestRunRequest, TestStatus};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

#[test]
fn runner_helpers_validate_and_run_commands_directly() {
    let valid = PytestRunRequest::witness();
    assert!(validate_request(&valid).is_ok());
    let mut missing_nodeid = valid.clone();
    missing_nodeid.nodeid.clear();
    assert!(matches!(
        validate_request(&missing_nodeid),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("node id")
    ));
    let mut missing_cwd = valid.clone();
    missing_cwd.cwd = PathBuf::new();
    assert!(matches!(
        validate_request(&missing_cwd),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("cwd")
    ));
    let mut missing_python = valid.clone();
    missing_python.python = PathBuf::new();
    assert!(matches!(
        validate_request(&missing_python),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("python")
    ));

    let mut cmd = Command::new(python!());
    cmd.arg("-c")
        .arg("print('runner helper')")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_command(cmd, &python!(), Some(Duration::from_secs(2))).unwrap();

    assert!(output.status.success());
    assert_eq!(
        TestStatus::from_exit_status(output.status),
        TestStatus::Passed
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("runner helper"));

    let mut no_timeout_cmd = Command::new(python!());
    no_timeout_cmd
        .arg("-c")
        .arg("print('no timeout')")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let no_timeout_output = run_command(no_timeout_cmd, &python!(), None).unwrap();
    assert!(no_timeout_output.status.success());
}

#[test]
fn subprocess_worker_sends_indexed_result() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = PytestRunRequest {
        nodeid: "test_sample.py::test_ok".to_string(),
        cwd: tmp.path().to_path_buf(),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    };
    let (tx, rx) = mpsc::channel();

    spawn_subprocess_job(3, req, tx);
    let (index, outcome) = rx.recv_timeout(Duration::from_secs(3)).unwrap();

    assert_eq!(index, 3);
    assert_eq!(outcome.unwrap().status, TestStatus::Passed);
}
