//! Subprocess SIGINT acceptance for `kiss test --watch`.

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
        .expect("spawn kiss test --watch");
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
        &["test", "--watch", "--lang", "python", "test_lib.py"],
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
    let daemon = start_watch_daemon(&["test", "--watch", "--lang", "rust", "."], tmp.path());
    wait_for_path(&tmp.path().join("BATCH_RUNNING"), Duration::from_secs(90));
    unsafe {
        assert_eq!(libc::kill(daemon.pid(), libc::SIGINT), 0);
    }
    assert_watch_interrupted_gone(daemon, Duration::from_secs(30));
}

#[test]
fn watch_daemon_parent_is_silent() {
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
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout={:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let watch_dir = tmp.path().join(".kiss").join("watch");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if read_watch_pid(&watch_dir).is_some() {
            break;
        }
        if Instant::now() >= deadline {
            panic!("daemon pid file not written after silent parent exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
