//! Background the watch process and prepare `.kiss/watch` error logs.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static WATCH_LOG_PATHS: Mutex<Option<WatchLogPaths>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub(crate) struct WatchLogPaths {
    pub log_path: PathBuf,
    pub pid_path: PathBuf,
}

pub(crate) fn watch_background_active() -> bool {
    WATCH_LOG_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
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
    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
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
            let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
            if libc::localtime_r(&t, tm.as_mut_ptr()).is_null() {
                return fallback_stamp();
            }
            let tm = tm.assume_init();
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
/// The pre-fork parent prints `Backgrounded, pid = <daemon_pid>` to stdout and exits 0.
/// Surviving process has stdin/stdout on `/dev/null` and stderr on the error log.
/// Call before any other CLI printing.
pub(crate) fn enter_watch_background() -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("kiss test requires a git repository ({e})"))?;
    {
        let guard = WATCH_LOG_PATHS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(paths) = guard.as_ref()
            && paths_belong_to_repo(paths, &repo_root)
        {
            return Ok(());
        }
        // Already backgrounded for another root: keep the daemon session as-is.
        if guard.is_some() && should_daemonize_watch() {
            return Ok(());
        }
    }
    let paths = prepare_watch_log_paths(&repo_root)?;
    if should_daemonize_watch() {
        daemonize_watch(&paths.log_path)?;
    }
    let mut guard = WATCH_LOG_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard
        .as_ref()
        .is_none_or(|existing| !paths_belong_to_repo(existing, &repo_root))
    {
        *guard = Some(paths);
    }
    Ok(())
}

fn paths_belong_to_repo(paths: &WatchLogPaths, repo_root: &Path) -> bool {
    let watch_dir = repo_root.join(".kiss").join("watch");
    paths
        .log_path
        .parent()
        .is_some_and(|parent| parent == watch_dir)
}

/// Return cached watch log paths for `repo_root`, creating them if missing.
///
/// Does not daemonize: backgrounding must happen only via `enter_watch_background`
/// before other CLI output. Replaces a cached entry that belongs to a different repo
/// (unit tests reuse one process across temporary repositories).
pub(crate) fn ensure_watch_log_paths(repo_root: &Path) -> Result<WatchLogPaths, String> {
    let mut guard = WATCH_LOG_PATHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(paths) = guard.as_ref()
        && paths_belong_to_repo(paths, repo_root)
    {
        if let Some(parent) = paths.log_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        return Ok(paths.clone());
    }
    let paths = prepare_watch_log_paths(repo_root)?;
    *guard = Some(paths.clone());
    Ok(paths)
}

/// Double-fork into a background session; stderr becomes `log_path`, stdout `/dev/null`.
///
/// The invoking parent waits for the daemon pid over a pipe, prints
/// `Backgrounded, pid = …`, then exits 0.
///
/// Marked `kiss-coverage-off`: double-fork children do not contribute LLVM coverage to the
/// parent process, so requiring line hits here would demand an unreachable denominator.
#[cfg(unix)]
#[doc = "kiss-coverage-off"]
pub(crate) fn daemonize_watch(log_path: &Path) -> Result<(), String> {
    let (read_fd, write_fd) = open_pid_pipe()?;
    match unsafe { libc::fork() } {
        -1 => {
            close_fd(read_fd);
            close_fd(write_fd);
            Err(format!("fork failed: {}", std::io::Error::last_os_error()))
        }
        0 => {
            close_fd(read_fd);
            finish_daemonize(write_fd, log_path)
        }
        _ => {
            close_fd(write_fd);
            parent_report_backgrounded(read_fd)
        }
    }
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn open_pid_pipe() -> Result<(i32, i32), String> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(format!("pipe failed: {}", std::io::Error::last_os_error()));
    }
    Ok((fds[0], fds[1]))
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn close_fd(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn parent_report_backgrounded(read_fd: i32) -> ! {
    let daemon_pid = read_daemon_pid(read_fd);
    close_fd(read_fd);
    if let Some(pid) = daemon_pid {
        println!("Backgrounded, pid = {pid}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }
    std::process::exit(1);
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn finish_daemonize(write_fd: i32, log_path: &Path) -> Result<(), String> {
    if unsafe { libc::setsid() } < 0 {
        close_fd(write_fd);
        return Err(format!("setsid failed: {}", std::io::Error::last_os_error()));
    }
    match unsafe { libc::fork() } {
        -1 => {
            close_fd(write_fd);
            Err(format!("fork failed: {}", std::io::Error::last_os_error()))
        }
        0 => report_pid_and_redirect(write_fd, log_path),
        _ => {
            close_fd(write_fd);
            std::process::exit(0);
        }
    }
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn report_pid_and_redirect(write_fd: i32, log_path: &Path) -> Result<(), String> {
    let pid = unsafe { libc::getpid() } as i32;
    if !write_daemon_pid(write_fd, pid) {
        close_fd(write_fd);
        return Err("failed to report background pid".into());
    }
    close_fd(write_fd);
    redirect_daemon_stdio(log_path)
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn read_daemon_pid(read_fd: i32) -> Option<i32> {
    let mut buf = [0u8; 4];
    let mut got = 0usize;
    while got < 4 {
        let n = unsafe { libc::read(read_fd, buf[got..].as_mut_ptr().cast(), 4 - got) };
        if n <= 0 {
            return None;
        }
        got += n as usize;
    }
    Some(i32::from_ne_bytes(buf))
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
fn write_daemon_pid(write_fd: i32, pid: i32) -> bool {
    let bytes = pid.to_ne_bytes();
    let mut sent = 0usize;
    while sent < 4 {
        let n = unsafe { libc::write(write_fd, bytes[sent..].as_ptr().cast(), 4 - sent) };
        if n <= 0 {
            return false;
        }
        sent += n as usize;
    }
    true
}

#[cfg(unix)]
#[doc = "kiss-coverage-off"]
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
#[doc = "kiss-coverage-off"]
pub(crate) fn daemonize_watch(log_path: &Path) -> Result<(), String> {
    let _ = log_path;
    Err("kiss test --watch background mode requires a Unix host".into())
}

pub(crate) fn should_daemonize_watch() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        if std::env::var_os("KISS_WATCH_FOREGROUND").is_some() {
            return false;
        }
        true
    }
}

#[cfg(test)]
#[path = "daemon_test.rs"]
mod tests;
