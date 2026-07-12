use std::io;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::sync::OnceLock;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessGroupIdentity {
    pub pid: u32,
    pub pgid: u32,
}

#[derive(Default)]
pub struct ProcessTreeRegistry {
    groups: Mutex<Vec<ProcessGroupIdentity>>,
}

impl ProcessTreeRegistry {
    pub fn record(&self, identity: ProcessGroupIdentity) {
        self.groups
            .lock()
            .expect("process tree registry lock")
            .push(identity);
    }

    pub fn identities(&self) -> Vec<ProcessGroupIdentity> {
        self.groups
            .lock()
            .expect("process tree registry lock")
            .clone()
    }

    pub fn residual_count(&self) -> usize {
        self.identities()
            .iter()
            .filter(|identity| process_group_alive(identity.pgid))
            .count()
    }
}

pub struct BatchProcessTreeGuard {
    registry: Arc<ProcessTreeRegistry>,
    interrupted: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    _subreaper: LinuxSubreaper,
}

impl BatchProcessTreeGuard {
    pub fn install() -> io::Result<Self> {
        let registry = Arc::new(ProcessTreeRegistry::default());
        let interrupted = Arc::new(AtomicBool::new(false));
        install_sigint_handler(Arc::clone(&registry), Arc::clone(&interrupted))?;
        Ok(Self {
            registry,
            interrupted,
            #[cfg(target_os = "linux")]
            _subreaper: LinuxSubreaper::install()?,
        })
    }

    #[allow(dead_code)]
    pub fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub fn registry(&self) -> Arc<ProcessTreeRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn spawn_batch_command(&self, command: &mut Command) -> io::Result<Child> {
        #[cfg(unix)]
        {
            let registry = Arc::clone(&self.registry);
            unsafe {
                command.pre_exec(move || {
                    libc::setpgid(0, 0);
                    let pgid = libc::getpgid(0);
                    if pgid > 0 {
                        registry.record(ProcessGroupIdentity {
                            pid: libc::getpid() as u32,
                            pgid: pgid as u32,
                        });
                    }
                    Ok(())
                });
            }
        }
        command.spawn()
    }

    pub fn terminate_descendants(&self, grace: Duration) -> usize {
        self.interrupted.store(true, Ordering::SeqCst);
        let identities = self.registry.identities();
        for identity in &identities {
            signal_process_group(identity.pgid, libc::SIGTERM);
        }
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.registry.residual_count() == 0 {
                return 0;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        for identity in &identities {
            signal_process_group(identity.pgid, libc::SIGKILL);
        }
        reap_zombies();
        self.registry.residual_count()
    }
}

impl Drop for BatchProcessTreeGuard {
    fn drop(&mut self) {
        clear_sigint_handler();
        let _ = self.terminate_descendants(Duration::from_millis(250));
    }
}

#[cfg(unix)]
static SIGINT_STATE: OnceLock<Mutex<Option<SigintHandlerState>>> = OnceLock::new();

#[cfg(unix)]
#[derive(Clone)]
struct SigintHandlerState {
    registry: Arc<ProcessTreeRegistry>,
    interrupted: Arc<AtomicBool>,
}

#[cfg(unix)]
extern "C" fn handle_sigint(_signal: libc::c_int) {
    if let Some(state) = SIGINT_STATE
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.as_ref().cloned())
    {
        state.interrupted.store(true, Ordering::SeqCst);
        for identity in state.registry.identities() {
            signal_process_group(identity.pgid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            if state.registry.residual_count() == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        for identity in state.registry.identities() {
            signal_process_group(identity.pgid, libc::SIGKILL);
        }
        reap_zombies();
    }
}

#[cfg(unix)]
fn install_sigint_handler(
    registry: Arc<ProcessTreeRegistry>,
    interrupted: Arc<AtomicBool>,
) -> io::Result<()> {
    let slot = SIGINT_STATE.get_or_init(|| Mutex::new(None));
    *slot
        .lock()
        .map_err(|_| io::Error::other("sigint registry lock poisoned"))? =
        Some(SigintHandlerState {
            registry,
            interrupted,
        });
    let previous = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_sigint as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        let mut old = std::mem::zeroed();
        let rc = libc::sigaction(libc::SIGINT, &action, &mut old);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        old.sa_sigaction
    };
    if previous == 0 {
        // Handler installed from default/ignored disposition.
    }
    Ok(())
}

fn clear_sigint_handler() {
    #[cfg(unix)]
    {
        if SIGINT_STATE.get().is_some() {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
            }
            if let Ok(mut slot) = SIGINT_STATE.get().expect("sigint registry").lock() {
                *slot = None;
            }
        }
    }
}

#[cfg(not(unix))]
fn install_sigint_handler(
    _registry: Arc<ProcessTreeRegistry>,
    _interrupted: Arc<AtomicBool>,
) -> io::Result<()> {
    Ok(())
}

pub fn record_child_process_group(registry: &ProcessTreeRegistry, child: &Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        if pid > 0 {
            let pgid = unsafe { libc::getpgid(pid as i32) };
            if pgid > 0 {
                registry.record(ProcessGroupIdentity {
                    pid,
                    pgid: pgid as u32,
                });
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (registry, child);
    }
}

fn signal_process_group(pgid: u32, signal: i32) {
    if pgid == 0 {
        return;
    }
    unsafe {
        libc::killpg(pgid as i32, signal);
    }
}

fn process_group_alive(pgid: u32) -> bool {
    if pgid == 0 {
        return false;
    }
    unsafe { libc::killpg(pgid as i32, 0) == 0 }
}

fn reap_zombies() {
    loop {
        let pid = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if pid <= 0 {
            break;
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxSubreaper;

#[cfg(target_os = "linux")]
impl LinuxSubreaper {
    fn install() -> io::Result<Self> {
        let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self)
    }
}

#[cfg(not(target_os = "linux"))]
struct LinuxSubreaper;

#[cfg(not(target_os = "linux"))]
impl LinuxSubreaper {
    fn install() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process-tree subreaper is only supported on Linux",
        ))
    }
}

#[cfg(test)]
#[path = "batch_process_tree_test.rs"]
mod tests;
