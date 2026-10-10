use std::fs;
use std::time::{Duration, Instant};

use crate::rpytest_runner::forkserver::ForkserverController;
use crate::rpytest_runner::forkserver_controller_runtime::SHUTDOWN_TIMEOUT;
use crate::rpytest_runner::forkserver_test_support::{base_req, test_python};

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
fn controller_start_sweeps_scratch_left_by_dead_controllers_only() {
    let mut exited = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = exited.id();
    exited.wait().unwrap();
    let base = std::env::temp_dir();
    let dead = base.join(format!("rpytest-forkserver-{dead_pid}-sweeptest"));
    let live = base.join(format!(
        "rpytest-forkserver-{}-sweeptest",
        std::process::id()
    ));
    for dir in [&dead, &live] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("rpytest-forkserver-stream-x"), b"{}\n").unwrap();
    }
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = base_req(tmp.path(), "test_sample.py::test_ok");

    let controller = ForkserverController::start(&test_python(), &req.bootstrap).unwrap();
    drop(controller);

    let dead_survived = dead.exists();
    let live_survived = live.exists();
    let _ = fs::remove_dir_all(&live);
    assert!(
        !dead_survived,
        "scratch of an exited controller must be swept"
    );
    assert!(live_survived, "scratch of a running process must be kept");
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
        elapsed < SHUTDOWN_TIMEOUT + Duration::from_millis(800),
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
