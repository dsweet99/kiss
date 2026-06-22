use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::forkserver::{
    ForkserverController, WireArtifact, WireRequest, WireResponse, duration_millis_u64,
    run_with_reused_controller, spawn_forkserver_worker,
};
use crate::{
    ForkserverPytestRunner, PytestRunError, PytestRunRequest, RequestedArtifact, TestStatus,
};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

fn passing_req(root: &std::path::Path, nodeid: &str) -> PytestRunRequest {
    PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: root.to_path_buf(),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    }
}

#[test]
fn forkserver_wire_request_preserves_contract_fields() {
    let mut env = BTreeMap::new();
    env.insert("A".to_string(), "B".to_string());
    let req = PytestRunRequest {
        nodeid: "test_sample.py::test_ok".to_string(),
        cwd: PathBuf::from("/tmp/project"),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env,
        preload_modules: vec!["preload_flag".to_string()],
        artifacts: vec![RequestedArtifact {
            name: "coverage".to_string(),
            path: PathBuf::from("coverage.json"),
        }],
        timeout: Some(Duration::from_millis(25)),
    };

    let wire = WireRequest::from_request(7, &req);

    assert_eq!(wire.id, 7);
    assert_eq!(wire.nodeid, req.nodeid);
    assert_eq!(wire.cwd, "/tmp/project");
    assert_eq!(wire.env["A"], "B");
    assert_eq!(wire.preload_modules, vec!["preload_flag"]);
    assert_eq!(wire.artifacts[0].name, "coverage");
    assert_eq!(wire.artifacts[0].path, "coverage.json");
    assert_eq!(wire.timeout_ms, Some(25));
    assert_eq!(WireArtifact::witness().name, "coverage");
    assert_eq!(duration_millis_u64(Duration::from_millis(9)), 9);
}

#[test]
fn forkserver_wire_response_status_contract() {
    assert_eq!(
        WireResponse::witness("passed").test_status().unwrap(),
        TestStatus::Passed
    );
    assert_eq!(
        WireResponse::witness("failed").test_status().unwrap(),
        TestStatus::Failed
    );
    assert!(matches!(
        WireResponse::witness("weird").test_status(),
        Err(PytestRunError::Protocol(message)) if message.contains("unknown test status")
    ));
}

#[test]
fn forkserver_run_many_bounded_limits_concurrency_and_preserves_order() {
    let tmp = tempfile::tempdir().unwrap();
    let state_path = tmp.path().join("active.txt");
    let max_path = tmp.path().join("active.txt.max");
    let lock_path = tmp.path().join("active.lock");
    fs::write(&state_path, "0").unwrap();
    fs::write(&max_path, "0").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        r#"
import fcntl
import os
import time

STATE = os.environ["STATE_PATH"]
LOCK = os.environ["LOCK_PATH"]
MAX_STATE = STATE + ".max"


def _read_int(path):
    try:
        with open(path) as f:
            return int(f.read() or "0")
    except FileNotFoundError:
        return 0


def _write_int(path, value):
    with open(path, "w") as f:
        f.write(str(value))


def _mark():
    with open(LOCK, "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        active = _read_int(STATE) + 1
        _write_int(STATE, active)
        _write_int(MAX_STATE, max(_read_int(MAX_STATE), active))
        fcntl.flock(lock, fcntl.LOCK_UN)
    try:
        time.sleep(0.12)
    finally:
        with open(LOCK, "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            _write_int(STATE, _read_int(STATE) - 1)
            fcntl.flock(lock, fcntl.LOCK_UN)


def test_a():
    _mark()


def test_b():
    _mark()


def test_c():
    _mark()
"#,
    )
    .unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "STATE_PATH".to_string(),
        state_path.to_string_lossy().to_string(),
    );
    env.insert(
        "LOCK_PATH".to_string(),
        lock_path.to_string_lossy().to_string(),
    );
    let req = |nodeid: &str| PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: tmp.path().to_path_buf(),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env: env.clone(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    };

    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            req("test_sample.py::test_a"),
            req("test_sample.py::test_b"),
            req("test_sample.py::test_c"),
        ],
        2,
    );

    let nodeids: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.as_ref().unwrap().nodeid.as_str())
        .collect();
    assert_eq!(
        nodeids,
        vec![
            "test_sample.py::test_a",
            "test_sample.py::test_b",
            "test_sample.py::test_c",
        ]
    );
    for outcome in &outcomes {
        let outcome = outcome.as_ref().unwrap();
        assert_eq!(
            outcome.status,
            TestStatus::Passed,
            "{}\nstdout:\n{}\nstderr:\n{}",
            outcome.nodeid,
            String::from_utf8_lossy(&outcome.stdout),
            String::from_utf8_lossy(&outcome.stderr)
        );
    }
    let max_active: usize = fs::read_to_string(max_path).unwrap().parse().unwrap();
    assert_eq!(max_active, 2);
}

#[test]
fn forkserver_timeout_releases_worker_for_next_request() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\n\
def test_sleep():\n    time.sleep(10)\n\n\
def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = |nodeid: &str, timeout| PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: tmp.path().to_path_buf(),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout,
    };

    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            req(
                "test_sample.py::test_sleep",
                Some(Duration::from_millis(30)),
            ),
            req("test_sample.py::test_ok", None),
        ],
        1,
    );

    assert_eq!(
        outcomes[0],
        Err(PytestRunError::Timeout(Duration::from_millis(30)))
    );
    let second = outcomes[1].as_ref().unwrap();
    assert_eq!(second.nodeid, "test_sample.py::test_ok");
    assert_eq!(second.status, TestStatus::Passed);
}

#[test]
fn forkserver_controller_run_executes_one_request_directly() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut controller = ForkserverController::start(&python!()).unwrap();

    let outcome = controller
        .run(passing_req(tmp.path(), "test_sample.py::test_ok"))
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
}

#[test]
fn forkserver_worker_processes_one_queued_request() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let queue = Arc::new(Mutex::new(VecDeque::from([(
        0,
        passing_req(tmp.path(), "test_sample.py::test_ok"),
    )])));
    let (tx, rx) = mpsc::channel();

    spawn_forkserver_worker(queue, tx);
    let (_index, outcome) = rx.recv_timeout(Duration::from_secs(3)).unwrap();

    assert_eq!(outcome.unwrap().status, TestStatus::Passed);
}

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
        passing_req(tmp.path(), "test_sample.py::test_ok"),
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
        .run_one(PytestRunRequest {
            nodeid: "test_sample.py::test_ok".to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string()],
            env,
            preload_modules: vec!["preload_flag".to_string()],
            artifacts: Vec::new(),
            timeout: None,
        })
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(fs::read_to_string(flag_path).unwrap(), "loaded");
}
