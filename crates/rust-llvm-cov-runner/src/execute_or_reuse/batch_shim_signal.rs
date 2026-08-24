use std::io;
#[cfg(unix)]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::execute_or_reuse::batch_process_tree::{
    ProcessGroupIdentity, signal_validated_process_group,
};

#[cfg(unix)]
static SHIM_DELEGATED_PGID: AtomicU32 = AtomicU32::new(0);

pub(crate) struct ShimSignalForwarder;

impl ShimSignalForwarder {
    pub(crate) fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            install_shim_signal_forwarder()?;
        }
        Ok(Self)
    }

    pub(crate) fn set_delegated_identity(identity: &ProcessGroupIdentity) {
        #[cfg(unix)]
        {
            SHIM_DELEGATED_PGID.store(identity.pgid, Ordering::SeqCst);
        }
        #[cfg(not(unix))]
        {
            let _ = identity;
        }
    }

    pub(crate) fn clear_delegated_identity() {
        #[cfg(unix)]
        {
            SHIM_DELEGATED_PGID.store(0, Ordering::SeqCst);
        }
    }
}

impl Drop for ShimSignalForwarder {
    fn drop(&mut self) {
        Self::clear_delegated_identity();
        #[cfg(unix)]
        {
            clear_shim_signal_forwarder();
        }
    }
}

#[cfg(unix)]
static SHIM_SIGNAL_STATE: OnceLock<std::sync::Mutex<bool>> = OnceLock::new();

#[cfg(test)]
#[cfg(unix)]
pub(crate) fn trigger_shim_forward_signal_for_test(signal: libc::c_int) {
    shim_forward_signal(signal);
}

#[cfg(unix)]
extern "C" fn shim_forward_signal(signal: libc::c_int) {
    let pgid = SHIM_DELEGATED_PGID.load(Ordering::SeqCst);
    if pgid == 0 {
        return;
    }
    let identity = ProcessGroupIdentity { pid: pgid, pgid };
    signal_validated_process_group(&identity, signal);
}

#[cfg(unix)]
pub(crate) fn install_shim_signal_forwarder() -> io::Result<()> {
    let slot = SHIM_SIGNAL_STATE.get_or_init(|| std::sync::Mutex::new(false));
    *slot
        .lock()
        .map_err(|_| io::Error::other("shim signal forwarder lock poisoned"))? = true;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let previous = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = shim_forward_signal as *const () as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            let mut old = std::mem::zeroed();
            let rc = libc::sigaction(signal, &action, &mut old);
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            old.sa_sigaction
        };
        let _ = previous;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn clear_shim_signal_forwarder() {
    if SHIM_SIGNAL_STATE.get().is_some() {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
        if let Ok(mut slot) = SHIM_SIGNAL_STATE.get().expect("shim signal state").lock() {
            *slot = false;
        }
    }
}
