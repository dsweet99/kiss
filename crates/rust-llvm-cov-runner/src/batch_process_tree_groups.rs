use super::ProcessGroupIdentity;

pub(crate) fn identity_still_valid(identity: &ProcessGroupIdentity) -> bool {
    if identity.pid == 0 || identity.pgid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let pid = identity.pid as i32;
        if unsafe { libc::kill(pid, 0) } != 0 {
            return false;
        }
        let pgid = unsafe { libc::getpgid(pid) };
        pgid > 0 && pgid as u32 == identity.pgid
    }
    #[cfg(not(unix))]
    {
        let _ = identity;
        false
    }
}

pub(crate) fn signal_validated_process_group(identity: &ProcessGroupIdentity, signal: i32) {
    if identity_still_valid(identity) {
        signal_process_group(identity.pgid, signal);
    }
}

pub(crate) fn signal_process_group(pgid: u32, signal: i32) {
    if pgid == 0 {
        return;
    }
    unsafe {
        libc::killpg(pgid as i32, signal);
    }
}

pub(crate) fn process_group_alive(pgid: u32) -> bool {
    if pgid == 0 {
        return false;
    }
    unsafe { libc::killpg(pgid as i32, 0) == 0 }
}
