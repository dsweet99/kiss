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
        child_preload_modules: Vec::new(),
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
        child_preload_modules: vec!["preload_flag".to_string()],
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
    assert_eq!(wire.child_preload_modules, vec!["preload_flag"]);
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
    use crate::bounded_concurrency_test_support::{
        assert_bounded_concurrency, setup_concurrency_fixture,
    };

    let fixture = setup_concurrency_fixture();
    assert_bounded_concurrency(
        &fixture,
        &[
            "test_sample.py::test_a",
            "test_sample.py::test_b",
            "test_sample.py::test_c",
        ],
        2,
        "forkserver",
        &|reqs, jobs| ForkserverPytestRunner::new().run_many_bounded(reqs, jobs),
    );
}

#[test]
fn forkserver_run_many_bounded_isolates_child_module_globals() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("stateful.py"), "VALUE = 0\n").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import stateful\n\n\
def test_mutate_global():\n    stateful.VALUE = 1\n    assert stateful.VALUE == 1\n\n\
def test_global_starts_clean():\n    assert stateful.VALUE == 0\n",
    )
    .unwrap();

    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            passing_req(tmp.path(), "test_sample.py::test_mutate_global"),
            passing_req(tmp.path(), "test_sample.py::test_global_starts_clean"),
        ],
        1,
    );

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
}

#[test]
fn forkserver_controller_pid_is_reused_for_multiple_requests() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let mut controller = ForkserverController::start(&python!()).unwrap();
    let pid = controller.controller_pid();

    controller
        .run(passing_req(tmp.path(), "test_sample.py::test_a"))
        .unwrap();
    controller
        .run(passing_req(tmp.path(), "test_sample.py::test_b"))
        .unwrap();

    assert_eq!(controller.controller_pid(), pid);
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
        child_preload_modules: Vec::new(),
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
fn forkserver_child_preloads_observe_request_specific_env() {
    let tmp = tempfile::tempdir().unwrap();
    let preload_dir = tmp.path().join("preloads");
    fs::create_dir(&preload_dir).unwrap();
    fs::write(
        preload_dir.join("preload_env.py"),
        "import os\n\
with open(os.environ['FLAG_PATH'], 'w') as f:\n    f.write(os.environ['PRELOAD_VALUE'])\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let request = |nodeid: &str, name: &str, value: &str| {
        let flag_path = tmp.path().join(name);
        let mut env = BTreeMap::new();
        env.insert(
            "PYTHONPATH".to_string(),
            preload_dir.to_string_lossy().to_string(),
        );
        env.insert(
            "FLAG_PATH".to_string(),
            flag_path.to_string_lossy().to_string(),
        );
        env.insert("PRELOAD_VALUE".to_string(), value.to_string());
        PytestRunRequest {
            nodeid: nodeid.to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string()],
            env,
            child_preload_modules: vec!["preload_env".to_string()],
            artifacts: Vec::new(),
            timeout: None,
        }
    };

    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            request("test_sample.py::test_a", "first.txt", "first"),
            request("test_sample.py::test_b", "second.txt", "second"),
        ],
        1,
    );

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(
        fs::read_to_string(tmp.path().join("first.txt")).unwrap(),
        "first"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("second.txt")).unwrap(),
        "second"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn forkserver_batch_drops_controller_after_workers_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let pid_path = tmp.path().join("controller_pid.txt");
    fs::write(
        tmp.path().join("preload_pid.py"),
        "import os\n\
with open(os.environ['CONTROLLER_PID_PATH'], 'w') as f:\n    f.write(str(os.getppid()))\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "PYTHONPATH".to_string(),
        tmp.path().to_string_lossy().to_string(),
    );
    env.insert(
        "CONTROLLER_PID_PATH".to_string(),
        pid_path.to_string_lossy().to_string(),
    );

    let outcome = ForkserverPytestRunner::new()
        .run_many_bounded(
            vec![PytestRunRequest {
                nodeid: "test_sample.py::test_ok".to_string(),
                cwd: tmp.path().to_path_buf(),
                python: python!(),
                pytest_args: vec!["-q".to_string()],
                env,
                child_preload_modules: vec!["preload_pid".to_string()],
                artifacts: Vec::new(),
                timeout: None,
            }],
            1,
        )
        .remove(0)
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    let pid = fs::read_to_string(pid_path).unwrap();
    let proc_path = PathBuf::from("/proc").join(pid.trim());
    for _ in 0..20 {
        if fs::metadata(&proc_path).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "forkserver controller still existed at {}",
        proc_path.display()
    );
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
fn forkserver_controller_run_reports_pytest_failure_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_fail():\n    assert False\n",
    )
    .unwrap();
    let mut controller = ForkserverController::start(&python!()).unwrap();

    let outcome = controller
        .run(passing_req(tmp.path(), "test_sample.py::test_fail"))
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.exit_code, Some(1));
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
            child_preload_modules: vec!["preload_flag".to_string()],
            artifacts: Vec::new(),
            timeout: None,
        })
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(fs::read_to_string(flag_path).unwrap(), "loaded");
}
