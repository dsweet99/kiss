use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{
    ForkserverPytestRunner, PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner,
    SubprocessPytestRunner, TestStatus, forkserver_pytest_runner, subprocess_pytest_runner,
};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

#[test]
fn run_many_preserves_request_order() {
    let runner = PytestRunner::from_fn(|req| {
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            artifacts: BTreeMap::new(),
        })
    });

    let cwd = PathBuf::from(".");
    let req = |nodeid: &str| PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: cwd.clone(),
        python: PathBuf::from("python"),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    };
    let got = runner.run_many(vec![req("a.py::test_a"), req("b.py::test_b")]);
    assert_eq!(got[0].as_ref().unwrap().nodeid, "a.py::test_a");
    assert_eq!(got[1].as_ref().unwrap().nodeid, "b.py::test_b");
}

#[test]
fn subprocess_runner_exposes_bounded_batch_api() {
    let got = SubprocessPytestRunner::new().run_many_bounded(Vec::new(), 1);

    assert!(got.is_empty());
}

#[test]
fn forkserver_runner_exposes_bounded_batch_api() {
    let got = ForkserverPytestRunner::new().run_many_bounded(Vec::new(), 1);

    assert!(got.is_empty());
}

#[test]
fn api_structs_expose_expected_fields() {
    let artifact = crate::RequestedArtifact::witness();
    assert_eq!(artifact.name, "coverage");
    assert_eq!(artifact.path, PathBuf::from("coverage.json"));
    assert_eq!(TestStatus::Passed, TestStatus::Passed);

    let req = PytestRunRequest::witness();
    assert_eq!(req.nodeid, "test_sample.py::test_ok");
    assert_eq!(req.pytest_args, vec!["-q"]);
    assert_eq!(req.env["A"], "B");
    assert_eq!(req.preload_modules, vec!["preload_mod"]);
    assert_eq!(req.artifacts[0].name, "coverage");
    assert_eq!(req.timeout, Some(Duration::from_secs(1)));

    let outcome = PytestRunOutcome::witness();
    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.stdout, b"out");
    assert_eq!(outcome.stderr, b"err");
    assert_eq!(
        outcome.artifacts["coverage"],
        PathBuf::from("coverage.json")
    );
}

#[test]
fn subprocess_runner_runs_one_pytest_node() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert 2 + 2 == 4\n\ndef test_other():\n    assert False\n",
    )
    .unwrap();

    let outcome = subprocess_pytest_runner()
        .run_one(PytestRunRequest {
            nodeid: "test_sample.py::test_ok".to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: None,
        })
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("1 passed"));
}

#[test]
fn forkserver_runner_runs_one_pytest_node() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    print('forkserver child ran')\n    assert 2 + 2 == 4\n",
    )
    .unwrap();

    let outcome = forkserver_pytest_runner()
        .run_one(PytestRunRequest {
            nodeid: "test_sample.py::test_ok".to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string(), "-s".to_string()],
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: None,
        })
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("forkserver child ran"));
}

#[test]
fn subprocess_runner_reports_failed_pytest_node() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_fail():\n    assert False\n",
    )
    .unwrap();

    let outcome = subprocess_pytest_runner()
        .run_one(PytestRunRequest {
            nodeid: "test_sample.py::test_fail".to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: None,
        })
        .unwrap();

    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.exit_code, Some(1));
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("1 failed"));
}

#[test]
fn subprocess_runner_run_many_preserves_order_for_real_nodes() {
    let ok_tmp = tempfile::tempdir().unwrap();
    let fail_tmp = tempfile::tempdir().unwrap();
    fs::write(
        ok_tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        fail_tmp.path().join("test_sample.py"),
        "def test_fail():\n    assert False\n",
    )
    .unwrap();
    let req = |root: &Path, nodeid: &str| PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: root.to_path_buf(),
        python: python!(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    };

    let outcomes = SubprocessPytestRunner.run_many(vec![
        req(ok_tmp.path(), "test_sample.py::test_ok"),
        req(fail_tmp.path(), "test_sample.py::test_fail"),
    ]);

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Failed);
}

#[test]
fn subprocess_runner_run_many_bounded_limits_concurrency_and_preserves_order() {
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


def read_int(path):
    try:
        with open(path) as f:
            return int(f.read() or "0")
    except FileNotFoundError:
        return 0


def write_int(path, value):
    with open(path, "w") as f:
        f.write(str(value))


def change_active(delta):
    with open(LOCK, "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        active = read_int(STATE) + delta
        assert active >= 0
        write_int(STATE, active)
        write_int(MAX_STATE, max(read_int(MAX_STATE), active))
        fcntl.flock(lock, fcntl.LOCK_UN)


def mark():
    change_active(1)
    try:
        time.sleep(0.08)
    finally:
        change_active(-1)


def test_a():
    mark()


def test_b():
    mark()


def test_c():
    mark()


def test_d():
    mark()
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

    let outcomes = SubprocessPytestRunner::new().run_many_bounded(
        vec![
            req("test_sample.py::test_a"),
            req("test_sample.py::test_b"),
            req("test_sample.py::test_c"),
            req("test_sample.py::test_d"),
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
            "test_sample.py::test_d",
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
fn subprocess_runner_imports_preload_before_pytest() {
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

    let outcome = subprocess_pytest_runner()
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

#[test]
fn subprocess_runner_enforces_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sleep.py"),
        "import time\n\ndef test_sleep():\n    time.sleep(10)\n",
    )
    .unwrap();

    let err = subprocess_pytest_runner()
        .run_one(PytestRunRequest {
            nodeid: "test_sleep.py::test_sleep".to_string(),
            cwd: tmp.path().to_path_buf(),
            python: python!(),
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: Some(Duration::from_millis(20)),
        })
        .unwrap_err();

    assert_eq!(err, PytestRunError::Timeout(Duration::from_millis(20)));
}

#[test]
fn invalid_request_rejects_missing_required_fields() {
    let valid = PytestRunRequest {
        nodeid: "test_x.py::test_x".to_string(),
        cwd: PathBuf::from("."),
        python: PathBuf::from("python"),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
        preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    };

    let mut missing_nodeid = valid.clone();
    missing_nodeid.nodeid.clear();
    assert!(matches!(
        subprocess_pytest_runner().run_one(missing_nodeid),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("node id")
    ));

    let mut missing_python = valid.clone();
    missing_python.python = PathBuf::new();
    assert!(matches!(
        subprocess_pytest_runner().run_one(missing_python),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("python")
    ));

    let mut missing_cwd = valid;
    missing_cwd.cwd = PathBuf::new();
    assert!(matches!(
        subprocess_pytest_runner().run_one(missing_cwd),
        Err(PytestRunError::InvalidRequest(message)) if message.contains("cwd")
    ));
}
