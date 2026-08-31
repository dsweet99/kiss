use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::rpytest_runner::{ForkserverPytestRunner, PytestRunError, PytestRunRequest, TestStatus};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

#[test]
fn forkserver_timeout_survives_child_sigalrm_ignore() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("disable_alarm.py"),
        "import signal\n\
signal.signal(signal.SIGALRM, signal.SIG_IGN)\n\
signal.setitimer(signal.ITIMER_REAL, 0)\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\ndef test_sleep():\n    time.sleep(10)\n",
    )
    .unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "PYTHONPATH".to_string(),
        tmp.path().to_string_lossy().to_string(),
    );
    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![PytestRunRequest::from_parts(
            "test_sample.py::test_sleep".to_string(),
            tmp.path().to_path_buf(),
            python!(),
            vec!["-q".to_string()],
            env,
            vec!["disable_alarm".to_string()],
            Vec::new(),
            Some(Duration::from_millis(40)),
        )],
        1,
    );
    assert_eq!(
        outcomes[0],
        Err(PytestRunError::Timeout(Duration::from_millis(40)))
    );
}

#[test]
fn forkserver_timeout_metamorphic_vs_untimed_sleep() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\ndef test_sleep():\n    time.sleep(0.2)\n",
    )
    .unwrap();
    let mk = |timeout| {
        PytestRunRequest::from_parts(
            "test_sample.py::test_sleep".to_string(),
            tmp.path().to_path_buf(),
            python!(),
            vec!["-q".to_string()],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            timeout,
        )
    };
    let timed = ForkserverPytestRunner::new()
        .run_many_bounded(vec![mk(Some(Duration::from_millis(30)))], 1);
    let untimed = ForkserverPytestRunner::new().run_many_bounded(vec![mk(None)], 1);
    assert_eq!(
        timed[0],
        Err(PytestRunError::Timeout(Duration::from_millis(30)))
    );
    assert_eq!(untimed[0].as_ref().unwrap().status, TestStatus::Passed);
}

#[test]
fn forkserver_same_module_timeout_does_not_contaminate_later_tests() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\n\
VALUE = 0\n\n\
def test_sleep():\n    global VALUE\n    VALUE = 1\n    time.sleep(10)\n\n\
def test_ok():\n    assert VALUE == 0\n",
    )
    .unwrap();
    let req = |nodeid: &str, timeout| {
        PytestRunRequest::from_parts(
            nodeid.to_string(),
            tmp.path().to_path_buf(),
            python!(),
            vec!["-q".to_string()],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            timeout,
        )
    };
    let outcomes = ForkserverPytestRunner::new().run_many_bounded(
        vec![
            req(
                "test_sample.py::test_sleep",
                Some(Duration::from_millis(50)),
            ),
            req("test_sample.py::test_ok", None),
        ],
        1,
    );
    assert_eq!(
        outcomes[0],
        Err(PytestRunError::Timeout(Duration::from_millis(50)))
    );
    let second = outcomes[1].as_ref().unwrap();
    assert_eq!(second.nodeid, "test_sample.py::test_ok");
    assert_eq!(second.status, TestStatus::Passed);
}
