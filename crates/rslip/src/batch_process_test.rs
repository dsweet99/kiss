use super::*;
use rpytest_runner::{PytestRunOutcome, PytestRunner, TestStatus};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn two_process_same_host_cold_cache_contention_executes_once_and_reuses_hit() {
    if env::var_os("RSLIP_TWO_PROCESS_CONTENTION_CHILD").is_some() {
        run_two_process_contention_child();
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    fs::create_dir(tmp.path().join("ready")).unwrap();
    let exe = env::current_exe().unwrap();
    let first = spawn_two_process_contention_child(&exe, tmp.path(), "first");
    let second = spawn_two_process_contention_child(&exe, tmp.path(), "second");

    wait_for_path(
        &tmp.path().join("ready").join("first"),
        Duration::from_secs(2),
    );
    wait_for_path(
        &tmp.path().join("ready").join("second"),
        Duration::from_secs(2),
    );
    fs::write(tmp.path().join("go"), b"go").unwrap();

    let first_output = first.wait_with_output().unwrap();
    let second_output = second.wait_with_output().unwrap();
    assert_child_success("first", &first_output);
    assert_child_success("second", &second_output);

    let execution_log = fs::read_to_string(tmp.path().join("executions.log")).unwrap();
    assert_eq!(
        execution_log.lines().count(),
        1,
        "expected one runner execution, got log:\n{execution_log}"
    );
    let mut statuses = vec![
        fs::read_to_string(tmp.path().join("result-first.txt")).unwrap(),
        fs::read_to_string(tmp.path().join("result-second.txt")).unwrap(),
    ];
    statuses.sort();
    assert_eq!(statuses, vec!["Hit", "MissStored"]);
}

fn spawn_two_process_contention_child(exe: &Path, root: &Path, child_id: &str) -> Child {
    Command::new(exe)
        .arg("--exact")
        .arg(
            "batch_process_test::two_process_same_host_cold_cache_contention_executes_once_and_reuses_hit",
        )
        .arg("--nocapture")
        .env("RSLIP_TWO_PROCESS_CONTENTION_CHILD", child_id)
        .env("RSLIP_TWO_PROCESS_CONTENTION_ROOT", root)
        .spawn()
        .unwrap()
}

fn run_two_process_contention_child() {
    let child_id = env::var("RSLIP_TWO_PROCESS_CONTENTION_CHILD").unwrap();
    let root = PathBuf::from(env::var_os("RSLIP_TWO_PROCESS_CONTENTION_ROOT").unwrap());
    fs::write(root.join("ready").join(&child_id), b"ready").unwrap();
    wait_for_path(&root.join("go"), Duration::from_secs(2));

    let execution_log = root.join("executions.log");
    let result_path = root.join(format!("result-{child_id}.txt"));
    let rslip = Rslip::new(PytestRunner::from_fn(move |req| {
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&execution_log)
            .unwrap();
        writeln!(log, "{}", std::process::id()).unwrap();
        thread::sleep(Duration::from_millis(750));
        let path = req.artifacts[0].path.clone();
        fs::write(&path, r#"{"files":{"/project/app.py":[1,3]}}"#).unwrap();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    }));

    let outcome = rslip
        .run_or_reuse_many_bounded(vec![rslip_sample_request(&root)], 1)
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
    fs::write(result_path, format!("{:?}", outcome.cache_status)).unwrap();
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < timeout,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_child_success(child_id: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{child_id} child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
