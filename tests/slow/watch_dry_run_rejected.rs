#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime};

use crate::support::watch_proc::start_watch_logged;

type KissSnapshot = BTreeMap<PathBuf, (SystemTime, Vec<u8>)>;

const DRY_RUN_ARGS: [&[&str]; 2] = [
    &["test", "--dry-run"],
    &["test", "--dry-run", "test_lib.py"],
];

fn kiss_cmd(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.args(args)
        .current_dir(dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("run kiss")
}

fn assert_dry_run_rejected(dir: &Path, phase: &str) {
    for args in DRY_RUN_ARGS {
        let t0 = Instant::now();
        let out = kiss_cmd(dir, args);
        let elapsed = t0.elapsed();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{phase}: {args:?} must be rejected; stderr={stderr:?}"
        );
        assert!(
            stderr.contains("unexpected argument '--dry-run'"),
            "{phase}: {args:?} stderr={stderr:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "{phase}: {args:?} must not wait on the watcher; elapsed={elapsed:?}"
        );
    }
}

fn read_log(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn request_lines(log: &Path) -> usize {
    read_log(log).matches("kiss test: request ").count()
}

fn wait_for_log(log: &Path, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !read_log(log).contains(needle) {
        assert!(
            Instant::now() < deadline,
            "watcher log never showed {needle:?}; log={:?}",
            read_log(log)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn snapshot_kiss_dir(root: &Path) -> KissSnapshot {
    let kiss = root.join(".kiss");
    let mut snap = BTreeMap::new();
    let mut stack = vec![kiss.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path == kiss.join("profraw") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if let (Ok(meta), Ok(bytes)) = (entry.metadata(), std::fs::read(&path)) {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                snap.insert(path, (mtime, bytes));
            }
        }
    }
    snap
}

fn assert_idle_dry_run_leaves_cache(root: &Path, log: &Path) {
    for _ in 0..10 {
        wait_for_log(log, "kiss test: Waiting");
        let log_before = read_log(log);
        let before = snapshot_kiss_dir(root);
        assert_dry_run_rejected(root, "idle watcher");
        let after = snapshot_kiss_dir(root);
        if read_log(log) == log_before {
            assert!(before == after, "rejected --dry-run changed .kiss/");
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("watcher never stayed idle across the --dry-run commands");
}

fn write_slow_fixture(root: &Path) {
    std::fs::write(
        root.join("test_lib.py"),
        "import time\nfrom lib import f\n\n\
         def test_f():\n    assert f() == 0\n\n\
         def test_f_slow():\n    time.sleep(6)\n    assert f() == 0\n",
    )
    .unwrap();
}

#[test]
fn dry_run_is_rejected_without_touching_watcher_or_cache() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = crate::common::fresh_seeded_python_watch_repo();
    let root = tmp.path();
    write_slow_fixture(root);
    assert_dry_run_rejected(root, "no watcher");

    let log_dir = tempfile::TempDir::new().unwrap();
    let log = log_dir.path().join("watch.log");
    let mut watch = start_watch_logged(root, &["test-watch"], &log);
    wait_for_log(&log, "kiss test: tests_remaining=");
    assert_dry_run_rejected(root, "watcher mid-cycle");
    assert!(
        !read_log(&log).contains("kiss test: Waiting"),
        "--dry-run commands must be sent while the startup cycle runs"
    );
    assert_eq!(
        request_lines(&log),
        0,
        "mid-cycle --dry-run reached the watcher"
    );
    wait_for_log(&log, "2 passed");

    assert_idle_dry_run_leaves_cache(root, &log);
    assert_eq!(request_lines(&log), 0, "idle --dry-run reached the watcher");

    let before = snapshot_kiss_dir(root);
    let out = kiss_cmd(root, &["test"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "plain kiss test after --dry-run; stdout={stdout:?}"
    );
    assert!(stdout.contains("2 passed"), "stdout={stdout:?}");
    assert_eq!(
        request_lines(&log),
        1,
        "plain kiss test must reach the watcher"
    );
    assert!(
        before != snapshot_kiss_dir(root),
        "snapshot must detect watcher writes"
    );
    assert!(watch.still_running(), "watcher must keep running");
}
