#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn spawn_oneshot(repo: &Path) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.env("PYTHONDONTWRITEBYTECODE", "1")
        .env("NO_COLOR", "1")
        .args(["test", "--lang", "python", "test_lib.py"])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kiss test")
}

fn collect_stdout(child: &mut Child) -> Arc<Mutex<String>> {
    let stdout = child.stdout.take().expect("kiss stdout");
    let buf = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buf);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            shared.lock().unwrap().push_str(&line);
            line.clear();
        }
    });
    buf
}

fn wait_contains(stdout: &Arc<Mutex<String>>, needle: &str, timeout: Duration) -> String {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let all = stdout.lock().unwrap().clone();
        if all.contains(needle) {
            return all;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let all = stdout.lock().unwrap().clone();
    panic!("timed out waiting for {needle:?}; stdout={all}");
}

#[test]
fn oneshot_waits_for_peer_kiss_test_on_tmp_repo() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    let mut first = spawn_oneshot(tmp.path());
    let first_out = collect_stdout(&mut first);
    wait_contains(&first_out, "kiss test: Planning", Duration::from_secs(15));
    let mut second = spawn_oneshot(tmp.path());
    let second_out = collect_stdout(&mut second);
    let waited = wait_contains(
        &second_out,
        "kiss test: waiting for kiss test",
        Duration::from_secs(15),
    );
    assert!(
        !waited.contains("kiss test: Planning"),
        "second oneshot must wait before planning; stdout={waited}"
    );
    let first_status = first.wait().expect("first oneshot");
    let second_status = second.wait().expect("second oneshot");
    let first_stderr = read_stderr(&mut first);
    let second_stderr = read_stderr(&mut second);
    assert!(
        first_status.success(),
        "first oneshot must succeed; stdout={} stderr={first_stderr}",
        first_out.lock().unwrap()
    );
    assert!(
        second_status.success(),
        "second oneshot must succeed after waiting; stdout={} stderr={second_stderr}",
        second_out.lock().unwrap()
    );
}

fn read_stderr(child: &mut Child) -> String {
    let Some(stderr) = child.stderr.take() else {
        return String::new();
    };
    let mut out = String::new();
    let _ = BufReader::new(stderr).read_to_string(&mut out);
    out
}
