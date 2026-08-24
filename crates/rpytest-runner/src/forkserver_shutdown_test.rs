use std::fs;
use std::time::{Duration, Instant};

use crate::forkserver::ForkserverController;
use crate::forkserver_controller_runtime::SHUTDOWN_TIMEOUT;
use crate::forkserver_test_support::{base_req, test_python};

#[test]
fn forkserver_shutdown_runs_pytest_unconfigure_once() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("unconfigure.txt");
    fs::write(
        tmp.path().join("conftest.py"),
        format!(
            "def pytest_unconfigure(config):\n    open(r'{path}', 'a').write('unconfigure\\n')\n",
            path = marker.display()
        ),
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = base_req(tmp.path(), "test_sample.py::test_ok");
    let mut controller = ForkserverController::start(&test_python(), &req.bootstrap).unwrap();
    controller.run(req).unwrap();
    controller.shutdown_graceful();
    let body = fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(body.matches("unconfigure").count(), 1, "{body}");
}

#[test]
fn forkserver_shutdown_force_kills_unresponsive_controller() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "import time\ndef pytest_unconfigure(config):\n    time.sleep(30)\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = base_req(tmp.path(), "test_sample.py::test_ok");
    let mut controller = ForkserverController::start(&test_python(), &req.bootstrap).unwrap();
    controller.run(req).unwrap();
    let pid = controller.controller_pid();
    let started = Instant::now();

    controller.shutdown_graceful();
    let elapsed = started.elapsed();
    assert!(
        elapsed >= SHUTDOWN_TIMEOUT,
        "expected wait at least {SHUTDOWN_TIMEOUT:?}, got {elapsed:?}"
    );
    assert!(
        elapsed < SHUTDOWN_TIMEOUT + Duration::from_secs(2),
        "force-kill took too long: {elapsed:?}"
    );
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "controller pid {pid} still alive after force-kill"
    );
}

#[test]
fn forkserver_shutdown_is_fast_when_unconfigure_is_slow() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("conftest.py"),
        "import time\ndef pytest_unconfigure(config):\n    time.sleep(30)\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = base_req(tmp.path(), "test_sample.py::test_ok");
    let mut controller = ForkserverController::start(&test_python(), &req.bootstrap).unwrap();
    controller.run(req).unwrap();
    let pid = controller.controller_pid();
    let started = Instant::now();
    controller.shutdown();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(200),
        "fast shutdown waited on unconfigure: {elapsed:?}"
    );
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "controller pid {pid} still alive after fast shutdown"
    );
}
