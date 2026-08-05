//! Background the watch process and prepare `.kiss/watch` error logs.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static WATCH_LOG_PATHS: OnceLock<WatchLogPaths> = OnceLock::new();

#[derive(Clone, Debug)]
pub(crate) struct WatchLogPaths {
    pub log_path: PathBuf,
    pub pid_path: PathBuf,
}

pub(crate) fn watch_background_active() -> bool {
    WATCH_LOG_PATHS.get().is_some()
}

pub(crate) fn prepare_watch_log_paths(repo_root: &Path) -> Result<WatchLogPaths, String> {
    let dir = repo_root.join(".kiss").join("watch");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stamp = watch_log_stamp();
    let log_path = dir.join(format!("{stamp}.log"));
    let pid_path = dir.join(format!("{stamp}.pid"));
    File::create(&log_path).map_err(|e| format!("cannot create {}: {e}", log_path.display()))?;
    Ok(WatchLogPaths { log_path, pid_path })
}

pub(crate) fn write_watch_pid(pid_path: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let mut f = File::create(pid_path)
        .map_err(|e| format!("cannot create {}: {e}", pid_path.display()))?;
    writeln!(f, "{pid}").map_err(|e| format!("cannot write {}: {e}", pid_path.display()))?;
    Ok(())
}

pub(crate) fn watch_log_stamp() -> String {
    #[cfg(unix)]
    {
        unsafe {
            let mut t: libc::time_t = 0;
            if libc::time(&mut t) == -1 {
                return fallback_stamp();
            }
            let tm = libc::localtime(&t);
            if tm.is_null() {
                return fallback_stamp();
            }
            let tm = *tm;
            format!(
                "{:04}{:02}{:02}.{:02}{:02}{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            )
        }
    }
    #[cfg(not(unix))]
    {
        fallback_stamp()
    }
}

fn fallback_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "0".into())
}

/// Validate the repo, open `.kiss/watch/*.log`, and double-fork into the background.
///
/// The pre-fork parent exits 0 with no output. Surviving process has stdin/stdout on
/// `/dev/null` and stderr on the error log. Call before any other CLI printing.
pub(crate) fn enter_watch_background() -> Result<(), String> {
    if WATCH_LOG_PATHS.get().is_some() {
        return Ok(());
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo_root = crate::test_git::assert_git_repo(&cwd)
        .and_then(|_| crate::test_git::git_repo_root(&cwd))
        .map_err(|e| format!("kiss test requires a git repository ({e})"))?;
    let paths = prepare_watch_log_paths(&repo_root)?;
    if should_daemonize_watch() {
        daemonize_watch(&paths.log_path)?;
    }
    let _ = WATCH_LOG_PATHS.set(paths);
    Ok(())
}

/// Ensure watch log paths exist (unit-test / late entry without early daemonize).
pub(crate) fn ensure_watch_log_paths(repo_root: &Path) -> Result<&'static WatchLogPaths, String> {
    if let Some(paths) = WATCH_LOG_PATHS.get() {
        return Ok(paths);
    }
    let paths = prepare_watch_log_paths(repo_root)?;
    if should_daemonize_watch() {
        daemonize_watch(&paths.log_path)?;
    }
    let _ = WATCH_LOG_PATHS.set(paths);
    WATCH_LOG_PATHS
        .get()
        .ok_or_else(|| "watch log paths unavailable".into())
}

/// Double-fork into a background session; stderr becomes `log_path`, stdout `/dev/null`.
#[cfg(unix)]
pub(crate) fn daemonize_watch(log_path: &Path) -> Result<(), String> {
    match unsafe { libc::fork() } {
        -1 => return Err(format!("fork failed: {}", std::io::Error::last_os_error())),
        0 => {}
        _ => std::process::exit(0),
    }
    if unsafe { libc::setsid() } < 0 {
        return Err(format!("setsid failed: {}", std::io::Error::last_os_error()));
    }
    match unsafe { libc::fork() } {
        -1 => return Err(format!("fork failed: {}", std::io::Error::last_os_error())),
        0 => {}
        _ => std::process::exit(0),
    }
    redirect_daemon_stdio(log_path)
}

#[cfg(unix)]
fn redirect_daemon_stdio(log_path: &Path) -> Result<(), String> {
    use std::os::unix::io::AsRawFd;

    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .map_err(|e| format!("cannot open /dev/null: {e}"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("cannot open {}: {e}", log_path.display()))?;
    let dups = [
        (null.as_raw_fd(), libc::STDIN_FILENO),
        (null.as_raw_fd(), libc::STDOUT_FILENO),
        (log.as_raw_fd(), libc::STDERR_FILENO),
    ];
    for (src, dst) in dups {
        if unsafe { libc::dup2(src, dst) } < 0 {
            return Err(format!("dup2 {dst} failed: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn daemonize_watch(log_path: &Path) -> Result<(), String> {
    let _ = log_path;
    Err("kiss test --watch background mode requires a Unix host".into())
}

pub(crate) fn should_daemonize_watch() -> bool {
    // In-process unit tests must not fork the test runner.
    if cfg!(test) {
        return false;
    }
    if std::env::var_os("KISS_WATCH_FOREGROUND").is_some() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
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
            // Parent path inside daemonize_watch calls process::exit(0).
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
    fn fallback_stamp_is_numeric() {
        let stamp = fallback_stamp();
        assert!(
            !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()),
            "{stamp}"
        );
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
        if !watch_background_active() {
            let _cwd = crate::cwd_test_lock::lock();
            let tmp = tempfile::tempdir().unwrap();
            crate::test_runner::test_mode_fixtures::init_git(&tmp);
            let orig = std::env::current_dir().unwrap();
            std::env::set_current_dir(tmp.path()).unwrap();
            enter_watch_background().unwrap();
            std::env::set_current_dir(orig).unwrap();
        }
        let paths = ensure_watch_log_paths(Path::new(".")).unwrap();
        assert!(paths.log_path.extension().is_some_and(|e| e == "log"));
    }
}
