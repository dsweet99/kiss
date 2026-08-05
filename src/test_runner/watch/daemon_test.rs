use super::*;

#[test]
fn prepare_watch_log_paths_creates_stamp_files() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = prepare_watch_log_paths(tmp.path()).unwrap();
    assert!(paths.log_path.starts_with(tmp.path().join(".kiss").join("watch")));
    assert!(paths.log_path.extension().is_some_and(|e| e == "log"));
    assert!(paths.pid_path.extension().is_some_and(|e| e == "pid"));
    assert!(paths.log_path.is_file());
    let name = paths.log_path.file_stem().unwrap().to_string_lossy();
    assert!(
        name.contains('.'),
        "expected YYYYMMDD.HHMMSS stem, got {name}"
    );
}

#[test]
fn write_watch_pid_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let pid_path = tmp.path().join("w.pid");
    write_watch_pid(&pid_path).unwrap();
    let body = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(body.trim(), std::process::id().to_string());
}

#[test]
fn write_watch_pid_creates_missing_parent_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let pid_path = tmp.path().join("nested").join("dir").join("w.pid");
    write_watch_pid(&pid_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&pid_path).unwrap().trim(),
        std::process::id().to_string()
    );
}

#[test]
fn should_daemonize_watch_false_under_unit_tests() {
    assert!(!should_daemonize_watch());
}

#[test]
fn watch_log_stamp_has_date_and_time() {
    let stamp = watch_log_stamp();
    let parts: Vec<_> = stamp.split('.').collect();
    assert_eq!(parts.len(), 2, "{stamp}");
    assert_eq!(parts[0].len(), 8, "{stamp}");
    assert_eq!(parts[1].len(), 6, "{stamp}");
}

#[test]
fn redirect_daemon_stdio_writes_stderr_to_log() {
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("redir.log");
    File::create(&log).unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        let code = match redirect_daemon_stdio(&log) {
            Ok(()) => {
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "watch-redirect-ok");
                let _ = std::io::stderr().flush();
                0
            }
            Err(_) => 2,
        };
        unsafe { libc::_exit(code) };
    }
    let mut status: libc::c_int = 0;
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid);
    assert!(libc::WIFEXITED(status), "status={status}");
    assert_eq!(libc::WEXITSTATUS(status), 0);
    let body = std::fs::read_to_string(&log).unwrap();
    assert!(
        body.contains("watch-redirect-ok"),
        "log body={body:?}"
    );
}

#[test]
fn daemonize_watch_first_parent_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("daemonize.log");
    File::create(&log).unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        // Parent path inside daemonize_watch prints Backgrounded and exits 0.
        let _ = daemonize_watch(&log);
        unsafe { libc::_exit(0) };
    }
    let mut status: libc::c_int = 0;
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid);
    assert!(libc::WIFEXITED(status), "status={status}");
    assert_eq!(libc::WEXITSTATUS(status), 0);
    // Best-effort cleanup of any surviving daemon grandchild.
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[test]
fn write_and_read_daemon_pid_round_trip() {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);
    assert!(write_daemon_pid(write_fd, 424242));
    unsafe {
        libc::close(write_fd);
    }
    assert_eq!(read_daemon_pid(read_fd), Some(424242));
    unsafe {
        libc::close(read_fd);
    }
}

#[test]
fn read_daemon_pid_returns_none_on_short_read() {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);
    unsafe {
        libc::close(write_fd);
    }
    assert_eq!(read_daemon_pid(read_fd), None);
    unsafe {
        libc::close(read_fd);
    }
}

#[test]
fn fallback_stamp_is_numeric() {
    let stamp = fallback_stamp();
    assert!(
        !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()),
        "{stamp}"
    );
}

#[test]
fn paths_belong_to_repo_rejects_foreign_watch_dir() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let paths = prepare_watch_log_paths(a.path()).unwrap();
    assert!(paths_belong_to_repo(&paths, a.path()));
    assert!(!paths_belong_to_repo(&paths, b.path()));
}

#[test]
fn ensure_watch_log_paths_replaces_foreign_repo_cache() {
    let _cwd = crate::cwd_test_lock::lock();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&first);
    crate::test_runner::test_mode_fixtures::init_git(&second);
    let paths_a = ensure_watch_log_paths(first.path()).unwrap();
    assert!(paths_belong_to_repo(&paths_a, first.path()));
    let paths_b = ensure_watch_log_paths(second.path()).unwrap();
    assert!(paths_belong_to_repo(&paths_b, second.path()));
    assert!(!paths_belong_to_repo(&paths_b, first.path()));
}

#[test]
fn enter_watch_background_ok_when_unset() {
    if watch_background_active() {
        return;
    }
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    enter_watch_background().unwrap();
    assert!(watch_background_active());
    enter_watch_background().unwrap();
    std::env::set_current_dir(orig).unwrap();
}

#[test]
fn ensure_watch_log_paths_returns_static_after_enter() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    if !watch_background_active() {
        enter_watch_background().unwrap();
    }
    let paths = ensure_watch_log_paths(tmp.path()).unwrap();
    assert!(paths.log_path.extension().is_some_and(|e| e == "log"));
    assert!(paths_belong_to_repo(&paths, tmp.path()));
    std::env::set_current_dir(orig).unwrap();
}
