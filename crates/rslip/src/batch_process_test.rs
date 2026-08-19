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
fn two_process_same_host_cold_cache_contention_prefers_first_store() {
    if env::var_os("RSLIP_TWO_PROCESS_CONTENTION_CHILD").is_some() {
        run_two_process_contention_child();
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    prepare_two_process_fixture(tmp.path());
    let (first, second) = spawn_contention_pair(tmp.path());
    wait_for_both_ready(tmp.path());
    fs::write(tmp.path().join("go"), b"go").unwrap();
    assert_contention_results(tmp.path(), first, second);
}

fn prepare_two_process_fixture(root: &Path) {
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    fs::write(root.join("test_sample.py"), "def test_ok():\n    assert True\n").unwrap();
    fs::create_dir(root.join("ready")).unwrap();
}

fn spawn_contention_pair(root: &Path) -> (Child, Child) {
    let exe = env::current_exe().unwrap();
    (
        spawn_two_process_contention_child(&exe, root, "first"),
        spawn_two_process_contention_child(&exe, root, "second"),
    )
}

fn wait_for_both_ready(root: &Path) {
    wait_for_path(&root.join("ready").join("first"), Duration::from_secs(2));
    wait_for_path(&root.join("ready").join("second"), Duration::from_secs(2));
}

fn assert_contention_results(root: &Path, first: Child, second: Child) {
    let first_output = first.wait_with_output().unwrap();
    let second_output = second.wait_with_output().unwrap();
    assert_child_success("first", &first_output);
    assert_child_success("second", &second_output);

    let execution_log = fs::read_to_string(root.join("executions.log")).unwrap();
    let executions = execution_log.lines().count();
    assert!(
        (1..=2).contains(&executions),
        "expected one or two runner executions, got log:\n{execution_log}"
    );
    let mut statuses = vec![
        fs::read_to_string(root.join("result-first.txt")).unwrap(),
        fs::read_to_string(root.join("result-second.txt")).unwrap(),
    ];
    statuses.sort();
    assert_eq!(statuses, vec!["Hit", "MissStored"]);
}

fn spawn_two_process_contention_child(exe: &Path, root: &Path, child_id: &str) -> Child {
    let mut command = Command::new(exe);
    configure_two_process_contention_child(&mut command, root, child_id);
    command.spawn().unwrap()
}

fn configure_two_process_contention_child<'a>(
    command: &'a mut Command,
    root: &Path,
    child_id: &str,
) -> &'a mut Command {
    command
        .arg("--exact")
        .arg(
            "batch_process_test::two_process_same_host_cold_cache_contention_prefers_first_store",
        )
        .arg("--nocapture")
        .env("RSLIP_TWO_PROCESS_CONTENTION_CHILD", child_id)
        .env("RSLIP_TWO_PROCESS_CONTENTION_ROOT", root)


        .env_remove("LLVM_PROFILE_FILE")
}

#[test]
fn contention_children_do_not_inherit_the_parent_coverage_profile() {
    let mut command = Command::new("rslip-test-child");
    configure_two_process_contention_child(&mut command, Path::new("/tmp/root"), "first");

    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| *key == "LLVM_PROFILE_FILE")
            .unwrap()
            .1,
        None
    );
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
        let app = req.cwd.join("app.py");
        let payload = format!(
            r#"{{"files":{{"{}":[1,3]}}}}"#,
            app.to_string_lossy().replace('\\', "/")
        );
        fs::write(&path, payload).unwrap();
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
