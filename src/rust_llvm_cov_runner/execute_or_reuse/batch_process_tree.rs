#[path = "batch_process_tree_groups.rs"]
mod batch_process_tree_groups;
#[path = "batch_process_tree_reap.rs"]
mod batch_process_tree_reap;

#[allow(unused_imports)]
pub(crate) use batch_process_tree_groups::signal_process_group;
pub(crate) use batch_process_tree_groups::{
    identity_still_valid, process_group_alive, signal_validated_process_group,
};

use std::io;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(test)]
pub(crate) fn signal_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessGroupIdentity {
    pub pid: u32,
    pub pgid: u32,
}

#[derive(Default)]
pub struct ProcessTreeRegistry {
    pub(crate) groups: Mutex<Vec<ProcessGroupIdentity>>,
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
    owns_sigint_handler: bool,
}

pub struct BatchScopeInterruptGuard;

impl BatchScopeInterruptGuard {
    pub fn install() -> io::Result<Self> {
        let registry = Arc::new(ProcessTreeRegistry::default());
        let interrupted = Arc::new(AtomicBool::new(false));
        register_batch_scope_sigint(Arc::clone(&registry), Arc::clone(&interrupted))?;
        install_sigint_handler(Arc::clone(&registry), Arc::clone(&interrupted))?;
        Ok(Self)
    }
}

impl Drop for BatchScopeInterruptGuard {
    fn drop(&mut self) {
        clear_batch_scope_sigint();
    }
}

pub fn batch_scope_interrupted() -> bool {
    batch_scope_sigint_state()
        .map(|(_, interrupted)| interrupted.load(Ordering::SeqCst))
        .unwrap_or(false)
}

pub fn cancel_active_batch_scope() {
    let Some((registry, interrupted)) = batch_scope_sigint_state() else {
        return;
    };
    interrupted.store(true, Ordering::SeqCst);
    let identities = registry.identities();
    for identity in &identities {
        signal_validated_process_group(identity, libc::SIGTERM);
    }
    if !identities.is_empty() {
        std::thread::sleep(Duration::from_millis(100));
    }
    for identity in &identities {
        signal_validated_process_group(identity, libc::SIGKILL);
    }
}

impl BatchProcessTreeGuard {
    pub fn install() -> io::Result<Self> {
        if let Some((registry, interrupted)) = batch_scope_sigint_state() {
            install_child_subreaper()?;
            return Ok(Self {
                registry,
                interrupted,
                owns_sigint_handler: false,
            });
        }
        let registry = Arc::new(ProcessTreeRegistry::default());
        let interrupted = Arc::new(AtomicBool::new(false));
        install_sigint_handler(Arc::clone(&registry), Arc::clone(&interrupted))?;
        install_child_subreaper()?;
        Ok(Self {
            registry,
            interrupted,
            owns_sigint_handler: true,
        })
    }

    #[allow(dead_code)]
    pub fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn set_interrupted_for_test(&self, value: bool) {
        self.interrupted.store(value, Ordering::SeqCst);
    }

    pub fn registry(&self) -> Arc<ProcessTreeRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn spawn_batch_command(&self, command: &mut Command) -> io::Result<Child> {
        #[cfg(unix)]
        {
            let registry = Arc::clone(&self.registry);
            unsafe {
                command.pre_exec(move || configure_batch_child_process_group(registry.as_ref()));
            }
        }
        command.spawn()
    }

    pub fn terminate_descendants(&self, grace: Duration) -> usize {
        if self.owns_sigint_handler {
            self.interrupted.store(true, Ordering::SeqCst);
        }
        self.reap_lingering_descendants(grace)
    }

    pub fn reap_lingering_descendants(&self, grace: Duration) -> usize {
        let identities = self.registry.identities();
        for identity in &identities {
            signal_validated_process_group(identity, libc::SIGTERM);
        }
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.registry.residual_count() == 0 {
                return 0;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        for identity in &identities {
            signal_validated_process_group(identity, libc::SIGKILL);
        }
        batch_process_tree_reap::reap_zombies();
        self.registry.residual_count()
    }
}

impl Drop for BatchProcessTreeGuard {
    fn drop(&mut self) {
        if self.owns_sigint_handler {
            clear_sigint_handler();
        }
        let _ = self.terminate_descendants(Duration::from_millis(250));
    }
}

#[cfg(unix)]
static SIGINT_STATE: OnceLock<Mutex<Option<SigintHandlerState>>> = OnceLock::new();

#[cfg(unix)]
static ACTIVE_SIGINT_FLAG: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());
#[cfg(unix)]
static ACTIVE_SIGINT_REGISTRY: AtomicPtr<ProcessTreeRegistry> =
    AtomicPtr::new(std::ptr::null_mut());

type BatchScopeSigintState = (Arc<ProcessTreeRegistry>, Arc<AtomicBool>);
static BATCH_SCOPE_SIGINT: OnceLock<Mutex<Option<BatchScopeSigintState>>> = OnceLock::new();

fn batch_scope_sigint_state() -> Option<(Arc<ProcessTreeRegistry>, Arc<AtomicBool>)> {
    BATCH_SCOPE_SIGINT
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
}

fn register_batch_scope_sigint(
    registry: Arc<ProcessTreeRegistry>,
    interrupted: Arc<AtomicBool>,
) -> io::Result<()> {
    let slot = BATCH_SCOPE_SIGINT.get_or_init(|| Mutex::new(None));
    *slot
        .lock()
        .map_err(|_| io::Error::other("batch scope sigint lock poisoned"))? =
        Some((registry, interrupted));
    Ok(())
}

fn clear_batch_scope_sigint() {
    clear_sigint_handler();
    if let Some(slot) = BATCH_SCOPE_SIGINT.get()
        && let Ok(mut state) = slot.lock()
    {
        *state = None;
    }
}

fn install_child_subreaper() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(unix)]
type SigintHandlerState = (Arc<ProcessTreeRegistry>, Arc<AtomicBool>);

#[cfg(unix)]
extern "C" fn handle_sigint(_signal: libc::c_int) {
    let flag = ACTIVE_SIGINT_FLAG.load(Ordering::SeqCst);
    if !flag.is_null() {
        unsafe {
            (*flag).store(true, Ordering::SeqCst);
        }
    }
    let registry = ACTIVE_SIGINT_REGISTRY.load(Ordering::SeqCst);
    if registry.is_null() {
        return;
    }
    let Some(identities) = (unsafe { &*registry })
        .groups
        .try_lock()
        .ok()
        .map(|groups| groups.clone())
    else {
        return;
    };
    for identity in &identities {
        signal_validated_process_group(identity, libc::SIGTERM);
    }
    for identity in &identities {
        signal_validated_process_group(identity, libc::SIGKILL);
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
        Some((Arc::clone(&registry), Arc::clone(&interrupted)));
    ACTIVE_SIGINT_FLAG.store(
        Arc::as_ptr(&interrupted) as *mut AtomicBool,
        Ordering::SeqCst,
    );
    ACTIVE_SIGINT_REGISTRY.store(
        Arc::as_ptr(&registry) as *mut ProcessTreeRegistry,
        Ordering::SeqCst,
    );
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_sigint as *const () as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        let mut old = std::mem::zeroed();
        let rc = libc::sigaction(libc::SIGINT, &action, &mut old);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    };
    Ok(())
}

#[cfg(not(unix))]
fn install_sigint_handler(
    registry: Arc<ProcessTreeRegistry>,
    interrupted: Arc<AtomicBool>,
) -> io::Result<()> {
    let _ = (registry, interrupted);
    Ok(())
}

fn clear_sigint_handler() {
    #[cfg(unix)]
    {
        ACTIVE_SIGINT_FLAG.store(std::ptr::null_mut(), Ordering::SeqCst);
        ACTIVE_SIGINT_REGISTRY.store(std::ptr::null_mut(), Ordering::SeqCst);
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

#[cfg(unix)]
fn configure_batch_child_process_group(registry: &ProcessTreeRegistry) -> std::io::Result<()> {
    if unsafe { libc::setpgid(0, 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    record_current_process_group(registry);
    Ok(())
}

#[cfg(unix)]
fn record_current_process_group(registry: &ProcessTreeRegistry) {
    let pgid = unsafe { libc::getpgid(0) };
    if pgid > 0 {
        registry.record(ProcessGroupIdentity {
            pid: std::process::id(),
            pgid: pgid as u32,
        });
    }
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

#[cfg(test)]
#[path = "batch_process_tree_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_process_tree_sigint_test.rs"]
mod sigint_tests;
