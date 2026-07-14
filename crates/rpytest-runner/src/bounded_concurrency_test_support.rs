use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{PytestRunOutcome, PytestRunRequest, TestStatus};

pub(crate) const CONCURRENCY_TEST_SCRIPT: &str = r#"
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
        time.sleep(0.2)
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
"#;

pub(crate) struct ConcurrencyFixture {
    pub tmp: tempfile::TempDir,
    pub state_path: PathBuf,
    pub max_path: PathBuf,
    pub env: BTreeMap<String, String>,
}

pub(crate) fn setup_concurrency_fixture() -> ConcurrencyFixture {
    let tmp = tempfile::tempdir().unwrap();
    let state_path = tmp.path().join("active.txt");
    let max_path = tmp.path().join("active.txt.max");
    let lock_path = tmp.path().join("active.lock");
    fs::write(&state_path, "0").unwrap();
    fs::write(&max_path, "0").unwrap();
    fs::write(tmp.path().join("test_sample.py"), CONCURRENCY_TEST_SCRIPT).unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "STATE_PATH".to_string(),
        state_path.to_string_lossy().to_string(),
    );
    env.insert(
        "LOCK_PATH".to_string(),
        lock_path.to_string_lossy().to_string(),
    );
    ConcurrencyFixture {
        tmp,
        state_path,
        max_path,
        env,
    }
}

pub(crate) fn concurrency_request(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    nodeid: &str,
) -> PytestRunRequest {
    PytestRunRequest {
        nodeid: nodeid.to_string(),
        cwd: cwd.to_path_buf(),
        python: PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string())),
        pytest_args: vec!["-q".to_string()],
        env: env.clone(),
        child_preload_modules: Vec::new(),
        artifacts: Vec::new(),
        timeout: None,
    }
}

pub(crate) fn reset_concurrency_counters(state_path: &Path, max_path: &Path) {
    fs::write(state_path, "0").unwrap();
    fs::write(max_path, "0").unwrap();
}

pub(crate) fn read_max_active(max_path: &Path) -> usize {
    fs::read_to_string(max_path).unwrap().parse().unwrap()
}

pub(crate) fn assert_passed_outcomes(outcomes: &[Result<PytestRunOutcome, crate::PytestRunError>]) {
    for outcome in outcomes {
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
}

pub(crate) fn assert_bounded_concurrency(
    fixture: &ConcurrencyFixture,
    nodeids: &[&str],
    jobs: usize,
    label: &str,
    run_bounded: &dyn Fn(
        Vec<PytestRunRequest>,
        usize,
    ) -> Vec<Result<PytestRunOutcome, crate::PytestRunError>>,
) {
    let requests: Vec<_> = nodeids
        .iter()
        .map(|nodeid| concurrency_request(fixture.tmp.path(), &fixture.env, nodeid))
        .collect();
    let expected: Vec<_> = nodeids.iter().map(|nodeid| nodeid.to_string()).collect();
    let mut max_active = 0usize;
    for attempt in 0..5 {
        reset_concurrency_counters(&fixture.state_path, &fixture.max_path);
        let outcomes = run_bounded(requests.clone(), jobs);
        let got: Vec<_> = outcomes
            .iter()
            .map(|outcome| outcome.as_ref().unwrap().nodeid.clone())
            .collect();
        assert_eq!(got, expected);
        assert_passed_outcomes(&outcomes);
        max_active = read_max_active(&fixture.max_path);
        if max_active >= jobs {
            break;
        }
        assert!(
            attempt + 1 < 5,
            "expected concurrent {label} workers, observed max_active={max_active}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(max_active, jobs);
}

impl ConcurrencyFixture {
    pub(crate) fn witness() -> Self {
        setup_concurrency_fixture()
    }
}
