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
fn subprocess_runner_clears_inherited_pytest_addopts() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_python = tmp.path().join("fake_python");
    fs::write(
        &fake_python,
        r#"#!/usr/bin/env python3
import json
import os
import sys

payload = {
    "PYTEST_ADDOPTS": os.environ.get("PYTEST_ADDOPTS"),
    "PYTEST_DISABLE_PLUGIN_AUTOLOAD": os.environ.get("PYTEST_DISABLE_PLUGIN_AUTOLOAD"),
}
open("observed.json", "w").write(json.dumps(payload))
raise SystemExit(0)
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&fake_python).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_python, perms).unwrap();
    }
    let old_addopts = std::env::var("PYTEST_ADDOPTS").ok();
    unsafe { std::env::set_var("PYTEST_ADDOPTS", "--testmon --testmon") };

    let outcome = crate::SubprocessPytestRunner::new()
        .run_one(PytestRunRequest::from_parts(
            "test_sample.py::test_ok".to_string(),
            tmp.path().to_path_buf(),
            fake_python,
            vec!["-q".to_string()],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .unwrap();

    match old_addopts {
        Some(value) => unsafe { std::env::set_var("PYTEST_ADDOPTS", value) },
        None => unsafe { std::env::remove_var("PYTEST_ADDOPTS") },
    }

    assert_eq!(outcome.status, TestStatus::Passed);
    let observed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join("observed.json")).unwrap())
            .unwrap();
    assert!(
        observed["PYTEST_ADDOPTS"].is_null(),
        "inherited PYTEST_ADDOPTS must be scrubbed: {observed}"
    );

    assert!(
        observed["PYTEST_DISABLE_PLUGIN_AUTOLOAD"].is_null(),
        "PYTEST_DISABLE_PLUGIN_AUTOLOAD must not be exported on the process env: {observed}"
    );
}

#[test]
fn subprocess_runner_clears_ini_addopts_unknown_without_plugins() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\naddopts = --random-order --full-trace --durations=10 --import-mode=importlib\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let outcome = crate::SubprocessPytestRunner::new()
        .run_one(PytestRunRequest::from_parts(
            "test_sample.py::test_ok".to_string(),
            tmp.path().to_path_buf(),
            python!(),
            vec!["-q".to_string()],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .unwrap();
    assert_eq!(outcome.status, TestStatus::Passed);
}

#[test]
fn subprocess_worker_sends_indexed_result() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = PytestRunRequest::from_parts(
        "test_sample.py::test_ok".to_string(),
        tmp.path().to_path_buf(),
        python!(),
        vec!["-q".to_string()],
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        None,
    );
    let (tx, rx) = mpsc::channel();

    spawn_subprocess_job(3, req, tx);
    let (index, outcome) = rx.recv_timeout(Duration::from_secs(3)).unwrap();

    assert_eq!(index, 3);
    assert_eq!(outcome.unwrap().status, TestStatus::Passed);
}
