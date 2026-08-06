use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::{ForkserverPytestRunner, PytestRunError, PytestRunRequest, TestStatus};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

/// Parent waitpid deadline must still time out when the child ignores SIGALRM.
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

/// Metamorphic: timeout vs no-timeout on the same sleeping node must disagree.
#[test]
fn forkserver_timeout_metamorphic_vs_untimed_sleep() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\ndef test_sleep():\n    time.sleep(0.2)\n",
    )
    .unwrap();
    let mk = |timeout| PytestRunRequest::from_parts(
        "test_sample.py::test_sleep".to_string(),
        tmp.path().to_path_buf(),
        python!(),
        vec!["-q".to_string()],
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        timeout,
    );
    let timed = ForkserverPytestRunner::new()
        .run_many_bounded(vec![mk(Some(Duration::from_millis(30)))], 1);
    let untimed = ForkserverPytestRunner::new().run_many_bounded(vec![mk(None)], 1);
    assert_eq!(
        timed[0],
        Err(PytestRunError::Timeout(Duration::from_millis(30)))
    );
    assert_eq!(untimed[0].as_ref().unwrap().status, TestStatus::Passed);
}
