//! Subprocess SIGINT acceptance for `kiss test --watch`.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

fn assert_watch_interrupted(status: std::process::ExitStatus) {
    use std::os::unix::process::ExitStatusExt;
    assert!(
        status.code() == Some(130) || status.signal() == Some(libc::SIGINT),
        "expected exit 130 or SIGINT, got {status:?}"
    );
}

/// Drain stdout on a background thread so closing the pipe does not SIGPIPE the child.
fn spawn_watch(args: &[&str], dir: &Path) -> (Child, mpsc::Receiver<()>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kiss test --watch");
    let stdout = child.stdout.take().expect("stdout piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("kiss test --watch") {
                let _ = tx.send(());
            }
        }
    });
    (child, rx)
}

fn wait_for_banner(rx: &mpsc::Receiver<()>, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(()) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(status)) = child.try_wait() {
                    panic!("kiss exited before watch banner: {status:?}");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait().ok();
                panic!("stdout closed before watch banner; status={status:?}");
            }
        }
    }
    let _ = child.kill();
    panic!("timed out waiting for kiss test --watch banner");
}

fn write_python_repo(root: &Path) {
    std::fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
    std::fs::write(
        root.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

fn write_rust_sleep_repo(root: &Path) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"watch_sigint\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    // Marker file lets the acceptance test wait until the Rust batch is in-flight.
    std::fs::write(
        root.join("src").join("lib.rs"),
        "#[test]\nfn sleeps() {\n    let _ = std::fs::write(\"BATCH_RUNNING\", b\"1\");\n    std::thread::sleep(std::time::Duration::from_secs(60));\n}\n",
    )
    .unwrap();
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_interrupted(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait().expect("wait after kill");
            panic!("timed out waiting for SIGINT exit; last status={status:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn watch_sigint_python_exits_130() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let (mut child, rx) = spawn_watch(
        &["test", "--watch", "--lang", "python", "test_lib.py"],
        tmp.path(),
    );
    wait_for_banner(&rx, &mut child, Duration::from_secs(20));
    std::thread::sleep(Duration::from_millis(200));
    unsafe {
        assert_eq!(libc::kill(child.id() as i32, libc::SIGINT), 0);
    }
    let status = wait_interrupted(&mut child, Duration::from_secs(10));
    assert_watch_interrupted(status);
}

#[test]
fn watch_sigint_rust_batch_exits_130() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_rust_sleep_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let (mut child, rx) = spawn_watch(&["test", "--watch", "--lang", "rust", "."], tmp.path());
    wait_for_banner(&rx, &mut child, Duration::from_secs(25));
    // Wait until the sleeping unit test is running so SIGINT hits an active Rust batch.
    wait_for_path(&tmp.path().join("BATCH_RUNNING"), Duration::from_secs(90));
    unsafe {
        assert_eq!(libc::kill(child.id() as i32, libc::SIGINT), 0);
    }
    let status = wait_interrupted(&mut child, Duration::from_secs(20));
    assert_watch_interrupted(status);
}
