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

#[cfg(target_os = "linux")]
pub(crate) fn kill_reparented_children() {
    let self_pid = std::process::id();
    let my_pgid = unsafe { libc::getpgrp() };
    let Ok(proc_dir) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in proc_dir.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if pid <= 1 || pid == self_pid as i32 {
            continue;
        }
        let stat_path = entry.path().join("stat");
        let Ok(stat_content) = std::fs::read_to_string(stat_path) else {
            continue;
        };
        let Some(idx) = stat_content.rfind(')') else {
            continue;
        };
        let rest = &stat_content[idx + 1..].trim_start();
        let mut fields = rest.split_whitespace();
        let _state = fields.next();
        let ppid = fields.next().and_then(|p| p.parse::<u32>().ok());
        if ppid == Some(self_pid) {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                if let Some(pgrp) = fields.next().and_then(|p| p.parse::<i32>().ok())
                    && pgrp > 1
                    && pgrp != my_pgid
                {
                    libc::kill(-pgrp, libc::SIGKILL);
                }
            }
        }
    }
}
