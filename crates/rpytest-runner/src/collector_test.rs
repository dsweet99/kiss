use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::collector::{
    PytestCollectError, PytestCollectRequest, SubprocessPytestCollector, normalize_nodeid,
    normalize_nodeids,
};

#[test]
fn normalize_nodeid_converts_absolute_paths_to_repo_relative() {
    let repo = Path::new("/repo");
    let got = normalize_nodeid("/repo/tests/test_app.py::test_value", repo).unwrap();
    assert_eq!(got, "tests/test_app.py::test_value");
}

#[test]
fn normalize_nodeid_rejects_escaping_paths() {
    let repo = Path::new("/repo");
    let err = normalize_nodeid("../outside.py::test_value", repo).unwrap_err();
    assert!(matches!(
        err,
        PytestCollectError::NodeidNormalization { .. }
    ));
}

#[test]
fn normalize_nodeid_rejects_absolute_paths_outside_repo() {
    let repo = Path::new("/repo");
    let err = normalize_nodeid("/other/tests/test_app.py::test_value", repo).unwrap_err();
    assert!(matches!(
        err,
        PytestCollectError::NodeidNormalization { .. }
    ));
}

#[test]
fn normalize_nodeids_preserves_order() {
    let repo = Path::new("/repo");
    let got = normalize_nodeids(
        &[
            "tests/a.py::test_a".to_string(),
            "/repo/tests/b.py::test_b".to_string(),
        ],
        repo,
    )
    .unwrap();
    assert_eq!(
        got,
        vec![
            "tests/a.py::test_a".to_string(),
            "tests/b.py::test_b".to_string(),
        ]
    );
}

#[test]
fn collect_subprocess_returns_pytest_nodeids_from_temp_repo() {
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_ok.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let python = PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()));
    let outcome = SubprocessPytestCollector::new()
        .collect(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(outcome.nodeids, vec!["tests/test_ok.py::test_ok"]);
}

#[test]
fn collect_pytest_nodeids_public_api_delegates_to_subprocess_collector() {
    let tmp = TempDir::new().unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_api.py"),
        "def test_api():\n    assert True\n",
    )
    .unwrap();
    let outcome = super::collect_pytest_nodeids(PytestCollectRequest {
        cwd: tmp.path().to_path_buf(),
        python: PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())),
        paths: Vec::new(),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
    })
    .unwrap();
    assert_eq!(outcome.nodeids, vec!["tests/test_api.py::test_api"]);
}

#[test]
fn collect_subprocess_rejects_invalid_request() {
    let err = SubprocessPytestCollector::new()
        .collect(PytestCollectRequest {
            cwd: PathBuf::new(),
            python: PathBuf::from("python"),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(err, PytestCollectError::InvalidRequest(_)));
}

#[test]
fn collect_subprocess_clears_pytest_addopts_and_disables_plugin_autoload() {
    let tmp = TempDir::new().unwrap();
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
sys.stdout.write("KISS_COLLECT_JSON:" + json.dumps({"nodeids": []}) + "\n")
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_python).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_python, perms).unwrap();
    let old_addopts = std::env::var("PYTEST_ADDOPTS").ok();
    unsafe { std::env::set_var("PYTEST_ADDOPTS", "--testmon") };

    let outcome = SubprocessPytestCollector::new()
        .collect(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: fake_python,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap();

    match old_addopts {
        Some(value) => unsafe { std::env::set_var("PYTEST_ADDOPTS", value) },
        None => unsafe { std::env::remove_var("PYTEST_ADDOPTS") },
    }

    let observed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join("observed.json")).unwrap())
            .unwrap();
    assert!(observed["PYTEST_ADDOPTS"].is_null());
    assert_eq!(
        observed["PYTEST_DISABLE_PLUGIN_AUTOLOAD"].as_str(),
        Some("1")
    );
    assert!(outcome.nodeids.is_empty());
}

#[test]
fn collect_subprocess_rejects_non_json_output() {
    let tmp = TempDir::new().unwrap();
    let fake_python = tmp.path().join("bad_collect");
    fs::write(
        &fake_python,
        r#"#!/usr/bin/env python3
print("not-json")
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_python).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_python, perms).unwrap();

    let err = SubprocessPytestCollector::new()
        .collect(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: fake_python,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(err, PytestCollectError::InvalidOutput(_)));
}

#[test]
fn collect_subprocess_fails_when_child_exits_nonzero_without_payload() {
    let tmp = TempDir::new().unwrap();
    let fake_python = tmp.path().join("failing_collect");
    fs::write(
        &fake_python,
        r#"#!/usr/bin/env python3
import sys
print("import error", file=sys.stderr)
raise SystemExit(2)
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_python).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_python, perms).unwrap();

    let err = SubprocessPytestCollector::new()
        .collect(PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: fake_python,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        err,
        PytestCollectError::CollectionFailed { .. } | PytestCollectError::InvalidOutput { .. }
    ));
}
