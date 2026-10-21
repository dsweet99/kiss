use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::rpytest_runner::PytestRunError;

#[cfg(target_os = "linux")]
pub(super) fn kill_on_parent_exit(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(die_if_parent_gone);
    }
}

#[cfg(target_os = "linux")]
fn die_if_parent_gone() -> std::io::Result<()> {
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::getppid() } == 1 {
        unsafe { libc::raise(libc::SIGKILL) };
    }
    Ok(())
}

pub(super) fn signal_pid(pid: u32) {
    #[cfg(unix)]
    {
        let Ok(raw) = libc::pid_t::try_from(pid) else {
            return;
        };
        if raw <= 0 {
            return;
        }
        unsafe { libc::kill(raw, libc::SIGKILL) };
        let deadline = std::time::Instant::now() + Duration::from_millis(150);
        while std::time::Instant::now() < deadline && process_alive(pid) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

pub(super) fn create_scratch_dir(cwd: &Path) -> Option<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let state = cwd
        .ancestors()
        .map(crate::test_state_dir)
        .find(|dir| dir.is_dir())?;
    let base = state.join("forkserver-scratch");
    sweep_dead_scratch_dirs(&base);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = base.join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn sweep_dead_scratch_dirs(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let owner = name
            .to_string_lossy()
            .split('-')
            .next()
            .and_then(|pid| pid.parse::<u32>().ok());
        if owner.is_some_and(|pid| !process_alive(pid)) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

pub(super) fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        let signalled = unsafe { libc::kill(pid, 0) == 0 };
        signalled || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        pid == std::process::id()
    }
}

fn active_forkservers() -> &'static Mutex<HashSet<u32>> {
    static ACTIVE: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn register_active_forkserver(pid: u32) {
    active_forkservers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(pid);
}

pub(super) fn unregister_active_forkserver(pid: u32) {
    active_forkservers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&pid);
}

pub fn cancel_active_forkservers() {
    #[cfg(unix)]
    {
        let pids: Vec<u32> = active_forkservers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect();
        for pid in &pids {
            signal_forkserver_group(*pid, libc::SIGTERM);
        }
        if !pids.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
        }
        for pid in pids {
            signal_forkserver_group(pid, libc::SIGKILL);
        }
    }
}

#[cfg(unix)]
fn signal_forkserver_group(pid: u32, signal: libc::c_int) {
    let pid = pid as libc::pid_t;
    if pid > 0 && unsafe { libc::getpgid(pid) } == pid {
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

pub(super) fn take_stdin(child: &mut Child) -> Result<ChildStdin, PytestRunError> {
    child
        .stdin
        .take()
        .ok_or_else(|| PytestRunError::Protocol("controller stdin unavailable".to_string()))
}

pub(super) fn take_stdout(child: &mut Child) -> Result<std::process::ChildStdout, PytestRunError> {
    child
        .stdout
        .take()
        .ok_or_else(|| PytestRunError::Protocol("controller stdout unavailable".to_string()))
}

pub(super) fn force_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
pub(super) fn wait_or_kill(child: &mut Child, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() >= deadline {
            force_reap(child);
            return;
        }
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                force_reap(child);
                return;
            }
        }
    }
}
