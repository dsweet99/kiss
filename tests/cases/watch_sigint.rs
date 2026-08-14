//! Subprocess SIGINT acceptance for foreground `kiss test --watch`.

#![cfg(unix)]

use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

struct WatchProc {
    child: Child,
}

impl WatchProc {
    fn pid(&self) -> i32 {
        self.child.id() as i32
    }
}

impl Drop for WatchProc {
    fn drop(&mut self) {
        let _ = unsafe { libc::kill(self.pid(), libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_watch_interrupted_gone(mut watch: WatchProc, timeout: Duration) {
    let pid = watch.pid();
    let deadline = Instant::now() + timeout;
    loop {
        // Prefer waitpid: a SIGINT-killed child is a zombie until reaped, and
        // `kill(pid, 0)` still succeeds on zombies.
        if let Ok(Some(status)) = watch.child.try_wait() {
            std::mem::forget(watch);
            assert!(
                status.code() == Some(130)
                    || status.signal() == Some(libc::SIGINT)
                    || !status.success(),
                "expected interrupted exit, got {status:?}"
            );
            return;
        }
        if Instant::now() >= deadline {
            drop(watch);
            panic!("timed out waiting for watch pid={pid} to exit after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[allow(clippy::zombie_processes)] // WatchProc::Drop always reaps the child.
fn start_watch(args: &[&str], dir: &Path) -> WatchProc {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kiss test --watch");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if session_ready(dir) {
            return WatchProc { child };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "timed out waiting for watch session under {}",
                dir.join(".kiss").join("watch").display()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn session_ready(repo: &Path) -> bool {
    let session = repo.join(".kiss").join("watch").join("session.json");
    session.is_file()
}

fn read_session_pid(repo: &Path) -> Option<u32> {
    let path = repo.join(".kiss").join("watch").join("session.json");
    let body = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("pid")?.as_u64().map(|p| p as u32)
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

#[test]
fn watch_sigint_python_exits_130() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let watch = start_watch(
        &["test", "--watch", "--lang", "python", "test_lib.py"],
        tmp.path(),
    );
    std::thread::sleep(Duration::from_millis(500));
    unsafe {
        assert_eq!(libc::kill(watch.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(watch, Duration::from_secs(30));
}

#[test]
fn watch_sigint_rust_batch_exits_130() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_rust_sleep_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let watch = start_watch(&["test", "--watch", "--lang", "rust", "."], tmp.path());
    wait_for_path(&tmp.path().join("BATCH_RUNNING"), Duration::from_secs(90));
    unsafe {
        assert_eq!(libc::kill(watch.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(watch, Duration::from_secs(30));
}

fn spawn_foreground_watch(dir: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--watch", "--lang", "python", "test_lib.py"])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kiss test --watch")
}

fn collect_stdout_until(child: &mut Child, needle_a: &str, needle_b: &str, timeout: Duration) -> String {
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    let mut stdout_pipe = child.stdout.take().expect("stdout");
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_reader = Arc::clone(&collected);
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout_pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            collected_reader
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    let deadline = Instant::now() + timeout;
    let stdout = loop {
        let snap = collected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if snap.contains(needle_a) && snap.contains(needle_b) {
            break snap;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!("timed out waiting for foreground watch progress; stdout={snap:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let pid = child.id() as i32;
    unsafe {
        assert_eq!(libc::kill(pid, libc::SIGINT), 0);
    }
    let _ = child.wait();
    let _ = reader.join();
    stdout
}

#[test]
fn watch_foreground_writes_session_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let watch = start_watch(
        &["test", "--watch", "--lang", "python", "test_lib.py"],
        tmp.path(),
    );
    let pid = read_session_pid(tmp.path()).expect("session pid");
    assert_eq!(pid, watch.pid() as u32);
    unsafe {
        assert_eq!(libc::kill(watch.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(watch, Duration::from_secs(30));
}

#[test]
fn watch_foreground_logs_planning_and_pass() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let mut child = spawn_foreground_watch(tmp.path());
    let stdout = collect_stdout_until(
        &mut child,
        "kiss test: Planning ...",
        "PASS:",
        Duration::from_secs(90),
    );
    assert!(
        stdout.contains("kiss test: Planning ..."),
        "foreground --watch must log planning; stdout={stdout:?}"
    );
    assert!(
        stdout.contains("PASS:"),
        "foreground --watch must log PASS/FAIL/TIMEOUT lines; stdout={stdout:?}"
    );
}

#[test]
fn second_watch_fails_while_first_alive() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let watch = start_watch(
        &["test", "--watch", "--lang", "python", "test_lib.py"],
        tmp.path(),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "--watch", "--lang", "python", "test_lib.py"])
        .current_dir(tmp.path())
        .output()
        .expect("second watch");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already running"),
        "stderr={stderr:?}"
    );
    unsafe {
        assert_eq!(libc::kill(watch.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(watch, Duration::from_secs(30));
}
