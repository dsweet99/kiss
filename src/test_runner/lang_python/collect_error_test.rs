use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

#[test]
fn pytest_collector_spawn_and_invalid_request_errors_are_actionable() {
    let tmp = TempDir::new().unwrap();
    let collector = rpytest_runner::subprocess_pytest_collector();
    let invalid = collector
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: PathBuf::new(),
            python: PathBuf::from("python"),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        invalid,
        rpytest_runner::PytestCollectError::InvalidRequest(_)
    ));
    let spawn = collector
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: PathBuf::from("/nonexistent/kiss-python-collector"),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        spawn,
        rpytest_runner::PytestCollectError::Spawn { .. }
    ));
    let empty_python = collector
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: PathBuf::new(),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        empty_python,
        rpytest_runner::PytestCollectError::InvalidRequest(_)
    ));
}

#[test]
fn pytest_collector_invalid_json_and_nodeid_errors_are_actionable() {
    let tmp = TempDir::new().unwrap();
    let fake_python = tmp.path().join("fake_invalid_json");
    fs::write(&fake_python, "#!/usr/bin/env python3\nprint('no marker')\n").unwrap();
    make_executable(&fake_python);
    let invalid_output = rpytest_runner::subprocess_pytest_collector()
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: fake_python.clone(),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        invalid_output,
        rpytest_runner::PytestCollectError::InvalidOutput(_)
    ));

    let bad_nodeid = tmp.path().join("fake_bad_nodeid");
    fs::write(
        &bad_nodeid,
        "#!/usr/bin/env python3\nimport json, sys\nsys.stdout.write('KISS_COLLECT_JSON:' + json.dumps({'nodeids': ['invalid-no-separator']}) + '\\n')\n",
    )
    .unwrap();
    make_executable(&bad_nodeid);
    let normalization = rpytest_runner::subprocess_pytest_collector()
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: bad_nodeid,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
        })
        .unwrap_err();
    assert!(matches!(
        normalization,
        rpytest_runner::PytestCollectError::NodeidNormalization { .. }
    ));
}

#[test]
fn pytest_collector_collection_failed_uses_stdout_when_stderr_empty() {
    let tmp = TempDir::new().unwrap();
    let fake_python = tmp.path().join("stdout_failure");
    fs::write(
        &fake_python,
        "#!/usr/bin/env python3\nimport sys\nprint('KISS_COLLECT_JSON:{\"nodeids\": []}')\nprint('failure detail')\nraise SystemExit(2)\n",
    )
    .unwrap();
    make_executable(&fake_python);
    let err = rpytest_runner::subprocess_pytest_collector()
        .collect(rpytest_runner::PytestCollectRequest {
            cwd: tmp.path().to_path_buf(),
            python: fake_python,
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: BTreeMap::from([("KISS_COLLECT_ENV".into(), "1".into())]),
        })
        .unwrap_err();
    let message = format!("{err:?}");
    assert!(message.contains("failure detail"));
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}
