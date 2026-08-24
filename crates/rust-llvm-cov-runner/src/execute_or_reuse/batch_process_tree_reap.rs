pub(crate) fn reap_zombies() {
    let _ = reap_zombies_count();
}

pub(crate) fn reap_zombies_count() -> usize {
    let mut reaped = 0;
    while reap_one_zombie() {
        reaped += 1;
    }
    reaped
}

fn reap_one_zombie() -> bool {
    let pid = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
    pid > 0
}
