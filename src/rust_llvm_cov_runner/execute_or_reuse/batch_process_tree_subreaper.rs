use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

static SUBREAPER_DEPTH: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn install_child_subreaper() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        SUBREAPER_DEPTH.fetch_add(1, Ordering::SeqCst);
    }
    Ok(())
}

pub(crate) fn clear_child_subreaper() {
    #[cfg(target_os = "linux")]
    {
        if SUBREAPER_DEPTH
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |depth| {
                depth.checked_sub(1)
            })
            .ok()
            == Some(1)
        {
            let _ = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 0, 0, 0, 0) };
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn child_subreaper_is_set() -> bool {
    let mut flag = 0;
    let rc = unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut flag, 0, 0, 0) };
    rc == 0 && flag != 0
}
