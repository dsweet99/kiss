//! Subprocess SIGINT acceptance for `kiss test --watch-bg`.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

struct WatchDaemon {
    pid: i32,
    child: Option<Child>,
}

impl WatchDaemon {
    fn pid(&self) -> i32 {
        self.pid
    }
}

impl Drop for WatchDaemon {
    fn drop(&mut self) {
        let _ = unsafe { libc::kill(self.pid, libc::SIGKILL) };
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn assert_watch_interrupted_gone(mut daemon: WatchDaemon, timeout: Duration) {
    let pid = daemon.pid();
    let deadline = Instant::now() + timeout;
    loop {
        let alive = unsafe { libc::kill(pid, 0) == 0 };
        if !alive {
            if let Some(mut child) = daemon.child.take() {
                let _ = child.wait();
            }
            std::mem::forget(daemon);
            return;
        }
        if Instant::now() >= deadline {
            drop(daemon);
            panic!("timed out waiting for watch daemon pid={pid} to exit after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn note_parent_exit(child: &mut Child, parent_exited_ok: &mut bool) {
    if *parent_exited_ok {
        return;
    }
    if let Ok(Some(status)) = child.try_wait() {
        assert!(
            status.success(),
            "watch parent should exit 0 after daemonize; status={status:?}"
        );
        *parent_exited_ok = true;
    }
}

fn start_watch_daemon(args: &[&str], dir: &Path) -> WatchDaemon {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kiss test --watch-bg");
    let mut parent_exited_ok = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(pid) = read_alive_watch_pid(dir) {
            note_parent_exit(&mut child, &mut parent_exited_ok);
            if parent_exited_ok || child.id() as i32 != pid {
                return WatchDaemon { pid, child: None };
            }
            return WatchDaemon {
                pid,
                child: Some(child),
            };
        }
        note_parent_exit(&mut child, &mut parent_exited_ok);
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "timed out waiting for watch pid under {}",
                dir.join(".kiss").join("watch").display()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn read_alive_watch_pid(repo: &Path) -> Option<i32> {
    let watch_dir = repo.join(".kiss").join("watch");
    let pid = read_watch_pid(&watch_dir)?;
    if unsafe { libc::kill(pid, 0) } == 0 {
        Some(pid)
    } else {
        None
    }
}

fn read_watch_pid(watch_dir: &Path) -> Option<i32> {
    let mut pids: Vec<PathBuf> = fs::read_dir(watch_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "pid"))
        .collect();
    pids.sort();
    let path = pids.last()?;
    let body = fs::read_to_string(path).ok()?;
    body.trim().parse().ok()
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

#[test]
fn watch_sigint_python_exits_130() {
    // Coverage runtimes can install SIGINT handlers that keep the process alive.
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let daemon = start_watch_daemon(
        &["test", "--watch-bg", "--lang", "python", "test_lib.py"],
        tmp.path(),
    );
    std::thread::sleep(Duration::from_millis(500));
    unsafe {
        assert_eq!(libc::kill(daemon.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(daemon, Duration::from_secs(30));
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
    let daemon = start_watch_daemon(&["test", "--watch-bg", "--lang", "rust", "."], tmp.path());
    wait_for_path(&tmp.path().join("BATCH_RUNNING"), Duration::from_secs(90));
    unsafe {
        assert_eq!(libc::kill(daemon.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(daemon, Duration::from_secs(30));
}

#[test]
fn watch_daemon_parent_reports_backgrounded_pid() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .env("KISS_WATCH_EXIT_AFTER_PID", "1")
        .args(["test", "--watch-bg", "--lang", "python", "test_lib.py"])
        .current_dir(tmp.path())
        .output()
        .expect("run kiss test --watch-bg");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim_end();
    let prefix = "Backgrounded, pid = ";
    assert!(
        stdout.starts_with(prefix),
        "stdout={stdout:?}"
    );
    let reported: i32 = stdout[prefix.len()..]
        .parse()
        .unwrap_or_else(|_| panic!("pid not an integer: {stdout:?}"));
    assert!(
        output.stderr.is_empty(),
        "stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let watch_dir = tmp.path().join(".kiss").join("watch");
    let deadline = Instant::now() + Duration::from_secs(30);
    let file_pid = loop {
        if let Some(pid) = read_watch_pid(&watch_dir) {
            break pid;
        }
        if Instant::now() >= deadline {
            panic!("daemon pid file not written after Backgrounded parent exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(reported, file_pid, "stdout pid must match .pid file");
    let _ = unsafe { libc::kill(file_pid, libc::SIGKILL) };
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
fn watch_foreground_does_not_print_backgrounded() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_repo(tmp.path());
    commit_all(tmp.path(), "init");
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .env("KISS_WATCH_EXIT_AFTER_PID", "1")
        .args(["test", "--watch", "--lang", "python", "test_lib.py"])
        .current_dir(tmp.path())
        .output()
        .expect("run kiss test --watch");
    assert!(
        output.status.success(),
        "stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Backgrounded"),
        "foreground --watch must not daemonize; stdout={stdout:?}"
    );
    let watch_dir = tmp.path().join(".kiss").join("watch");
    assert!(
        read_watch_pid(&watch_dir).is_some(),
        "foreground --watch should still write a pid file"
    );
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
