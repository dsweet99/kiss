use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::{ForkserverPytestRunner, PytestRunRequest, TestStatus};

macro_rules! python {
    () => {
        PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
    };
}

#[test]
fn forkserver_duration_tracks_pytest_call_not_child_wall() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "import time\n\ndef test_sleep():\n    time.sleep(0.2)\n",
    )
    .unwrap();
    let req = PytestRunRequest::from_parts(
        "test_sample.py::test_sleep".to_string(),
        tmp.path().to_path_buf(),
        python!(),
        vec!["-q".to_string()],
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        None,
    );
    let wall = std::time::Instant::now();
    let outcome = ForkserverPytestRunner::new().run_one(req).unwrap();
    let wall_ms = wall.elapsed().as_millis();
    assert_eq!(outcome.status, TestStatus::Passed);
    let test_ms = outcome.duration.as_millis();
    assert!(
        test_ms >= 150 && test_ms < 500,
        "expected pytest call duration near 200ms, got {test_ms}ms (wall {wall_ms}ms)"
    );
    assert!(
        test_ms + 50 < wall_ms || wall_ms < 500,
        "test duration {test_ms}ms should not include full child wall {wall_ms}ms"
    );
}
