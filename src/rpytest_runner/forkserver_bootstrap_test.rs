use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use crate::rpytest_runner::forkserver::{ForkserverController, run_with_reused_controller};
use crate::rpytest_runner::forkserver_test_support::{base_req, test_python};
use crate::rpytest_runner::{
    ForkserverPytestRunner, PytestBootstrap, PytestRunError, PytestRunRequest, TestStatus,
    parent_safe_env,
};

#[test]
fn parent_safe_env_strips_selector_specific_keys() {
    let mut env = BTreeMap::new();
    env.insert("PYTHONPATH".to_string(), "/proj".to_string());
    env.insert("RSLIP_COVERAGE_OUT".to_string(), "/tmp/a.json".to_string());
    env.insert("TESTMON_DATAFILE".to_string(), "/tmp/t.db".to_string());
    let safe = parent_safe_env(&env);
    assert_eq!(safe.get("PYTHONPATH").map(String::as_str), Some("/proj"));
    assert!(!safe.contains_key("RSLIP_COVERAGE_OUT"));
    assert!(!safe.contains_key("TESTMON_DATAFILE"));
}

#[test]
fn forkserver_configures_pytest_once_per_controller() {
    let tmp = tempfile::tempdir().unwrap();
    let counter = tmp.path().join("counter.txt");
    fs::write(
        tmp.path().join("conftest.py"),
        "import os\nCOUNTER = os.environ['COUNTER_PATH']\ndef pytest_configure(config):\n    with open(COUNTER, 'a') as f:\n        f.write('configure\\n')\ndef pytest_sessionstart(session):\n    with open(COUNTER, 'a') as f:\n        f.write('sessionstart\\n')\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\ndef test_b():\n    assert True\n",
    )
    .unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "COUNTER_PATH".to_string(),
        counter.to_string_lossy().to_string(),
    );
    let req = |nodeid: &str| {
        PytestRunRequest::from_parts(
            nodeid.to_string(),
            tmp.path().to_path_buf(),
            test_python(),
            vec!["-q".to_string()],
            env.clone(),
            Vec::new(),
            Vec::new(),
            None,
        )
    };
    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![req("test_sample.py::test_a"), req("test_sample.py::test_b")],
        1,
    );
    assert!(
        outcomes[0].is_ok(),
        "first outcome: {:?}",
        outcomes[0].as_ref().err()
    );
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
    let body = fs::read_to_string(&counter).unwrap();
    assert_eq!(
        body.matches("configure").count(),
        1,
        "expected one configure, got:\n{body}"
    );
    assert_eq!(
        body.matches("sessionstart").count(),
        2,
        "expected two sessionstart, got:\n{body}"
    );
}

#[test]
fn forkserver_isolation_with_module_loaded_during_parent_configure() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("stateful.py"), "VALUE = 0\n").unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "import stateful  # loaded during parent configure\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import stateful\n\n\
def test_mutate_global():\n    stateful.VALUE = 1\n    assert stateful.VALUE == 1\n\n\
def test_global_starts_clean():\n    assert stateful.VALUE == 0\n",
    )
    .unwrap();
    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            base_req(tmp.path(), "test_sample.py::test_mutate_global"),
            base_req(tmp.path(), "test_sample.py::test_global_starts_clean"),
        ],
        1,
    );
    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
}

#[test]
fn forkserver_bootstrap_identity_reuses_or_restarts_controller() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut controller = None;
    let first = base_req(tmp.path(), "test_sample.py::test_ok");
    run_with_reused_controller(&mut controller, first.clone()).unwrap();
    let pid1 = controller.as_ref().unwrap().controller_pid();

    run_with_reused_controller(&mut controller, first.clone()).unwrap();
    assert_eq!(controller.as_ref().unwrap().controller_pid(), pid1);

    let mut different = first;
    different.bootstrap = PytestBootstrap::new(
        different.cwd.clone(),
        vec![
            "-q".to_string(),
            "-p".to_string(),
            "no:cacheprovider".to_string(),
        ],
        BTreeMap::new(),
    );
    different.pytest_args = different.bootstrap.pytest_args.clone();
    run_with_reused_controller(&mut controller, different).unwrap();
    assert_ne!(controller.as_ref().unwrap().controller_pid(), pid1);
}

#[test]
fn forkserver_bootstrap_rejects_plugin_that_starts_a_thread() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "import threading\nimport time\ndef pytest_configure(config):\n    threading.Thread(target=lambda: time.sleep(30), daemon=True).start()\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let started = ForkserverController::start(
        &test_python(),
        &base_req(tmp.path(), "test_sample.py::test_ok").bootstrap,
    );
    let err = match started {
        Ok(_) => panic!("expected bootstrap to fail when a plugin starts a thread"),
        Err(error) => error,
    };
    match err {
        PytestRunError::Protocol(message) => {
            assert!(
                message.contains("non-main Python threads") || message.contains("fork-unsafe"),
                "{message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn forkserver_bootstrap_rejects_unsupported_pytest_major_version() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut env = BTreeMap::new();
    env.insert("RPYTEST_FORKSERVER_FAKE_MAJOR".to_string(), "7".to_string());
    let bootstrap = PytestBootstrap::new(tmp.path().to_path_buf(), vec!["-q".to_string()], env);
    let started = ForkserverController::start(&test_python(), &bootstrap);
    let err = match started {
        Ok(_) => panic!("expected bootstrap to fail for unsupported pytest major"),
        Err(error) => error,
    };
    match err {
        PytestRunError::Protocol(message) => {
            assert!(
                message.contains("unsupported pytest major version"),
                "{message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn forkserver_bootstrap_rejects_configure_exception() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "def pytest_configure(config):\n    raise RuntimeError('boom-configure')\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let started = ForkserverController::start(
        &test_python(),
        &base_req(tmp.path(), "test_sample.py::test_ok").bootstrap,
    );
    let err = match started {
        Ok(_) => panic!("expected bootstrap to fail on configure exception"),
        Err(error) => error,
    };
    match err {
        PytestRunError::Protocol(message) => {
            assert!(
                message.contains("bootstrap failed") && message.contains("boom-configure"),
                "{message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn forkserver_bootstrap_clears_ini_addopts_unknown_without_plugins() {
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
    let outcomes = ForkserverPytestRunner::new()
        .run_many_bounded(vec![base_req(tmp.path(), "test_sample.py::test_ok")], 1);
    assert!(
        outcomes[0].is_ok(),
        "bootstrap must tolerate ini addopts without autoloaded plugins: {:?}",
        outcomes[0].as_ref().err()
    );
    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
}

#[test]
fn configured_parent_path_is_faster_than_two_cold_subprocess_configures() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "import time\ndef pytest_configure(config):\n    time.sleep(0.12)\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\ndef test_b():\n    assert True\n",
    )
    .unwrap();
    let reqs = vec![
        base_req(tmp.path(), "test_sample.py::test_a"),
        base_req(tmp.path(), "test_sample.py::test_b"),
    ];
    let started = Instant::now();
    let fork_out = ForkserverPytestRunner::new().run_many_bounded(reqs.clone(), 1);
    let fork_elapsed = started.elapsed();
    for outcome in &fork_out {
        assert_eq!(outcome.as_ref().unwrap().status, TestStatus::Passed);
    }

    let sub_out = crate::rpytest_runner::SubprocessPytestRunner::new().run_many_bounded(reqs, 1);
    for outcome in &sub_out {
        assert_eq!(outcome.as_ref().unwrap().status, TestStatus::Passed);
    }
    let fork_duration: Duration = fork_out
        .iter()
        .map(|outcome| outcome.as_ref().unwrap().duration)
        .sum();
    let sub_duration: Duration = sub_out
        .iter()
        .map(|outcome| outcome.as_ref().unwrap().duration)
        .sum();
    // Each cold subprocess invocation pays pytest_configure sleep; forkserver pays once in parent.
    for outcome in &sub_out {
        assert!(
            outcome.as_ref().unwrap().duration >= Duration::from_millis(90),
            "cold subprocess should include configure sleep: {:?}",
            outcome.as_ref().unwrap().duration
        );
    }
    assert!(
        sub_duration > fork_duration + Duration::from_millis(80),
        "two cold subprocess configures ({sub_duration:?}) should exceed forkserver per-run time ({fork_duration:?})"
    );
    assert!(fork_elapsed > Duration::from_millis(50));
}

#[test]
fn forkserver_empty_selection_and_collection_error_preserve_exit_codes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    fs::write(tmp.path().join("test_bad.py"), "def test_bad(\n").unwrap();
    let mut controller = ForkserverController::start(
        &test_python(),
        &base_req(tmp.path(), "test_sample.py::test_ok").bootstrap,
    )
    .unwrap();

    let empty = controller
        .run(base_req(tmp.path(), "test_sample.py::test_missing"))
        .unwrap();
    assert_eq!(empty.status, TestStatus::Failed);
    assert_eq!(empty.exit_code, Some(4), "USAGE_ERROR for missing nodeid");

    let collect_err = controller
        .run(base_req(tmp.path(), "test_bad.py::test_bad"))
        .unwrap();
    assert_eq!(collect_err.status, TestStatus::Failed);
    assert_eq!(
        collect_err.exit_code,
        Some(4),
        "USAGE_ERROR for collection failure"
    );

    let ok = controller
        .run(base_req(tmp.path(), "test_sample.py::test_ok"))
        .unwrap();
    assert_eq!(ok.status, TestStatus::Passed);
    assert_eq!(ok.exit_code, Some(0));
}

#[test]
fn forkserver_lifecycle_errors_match_pytest_exit_codes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_life.py"),
        "import pytest\ndef test_ok():\n    assert True\ndef test_sys():\n    raise SystemExit(99)\ndef test_exit():\n    pytest.exit('bye', returncode=7)\n",
    )
    .unwrap();
    let mut controller = ForkserverController::start(
        &test_python(),
        &base_req(tmp.path(), "test_life.py::test_ok").bootstrap,
    )
    .unwrap();

    let sys_exit = controller
        .run(base_req(tmp.path(), "test_life.py::test_sys"))
        .unwrap();
    assert_eq!(sys_exit.status, TestStatus::Failed);
    assert_eq!(sys_exit.exit_code, Some(1), "SystemExit in test body");

    let interrupted = controller
        .run(base_req(tmp.path(), "test_life.py::test_exit"))
        .unwrap();
    assert_eq!(interrupted.status, TestStatus::Failed);
    assert_eq!(interrupted.exit_code, Some(7), "pytest.exit returncode");

    let boom_dir = tmp.path().join("boom");
    fs::create_dir(&boom_dir).unwrap();
    fs::write(
        boom_dir.join("conftest.py"),
        "def pytest_sessionstart(session):\n    raise RuntimeError('boom-sessionstart')\n",
    )
    .unwrap();
    fs::write(
        boom_dir.join("test_ok.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut boom = ForkserverController::start(
        &test_python(),
        &base_req(&boom_dir, "test_ok.py::test_ok").bootstrap,
    )
    .unwrap();
    let unexpected = boom
        .run(base_req(&boom_dir, "test_ok.py::test_ok"))
        .unwrap();
    assert_eq!(unexpected.status, TestStatus::Failed);
    assert_eq!(unexpected.exit_code, Some(3), "INTERNAL_ERROR");
}
