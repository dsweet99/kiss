use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct WatchProc {
    child: Child,
}

impl Drop for WatchProc {
    fn drop(&mut self) {
        let pid = self.child.id() as i32;
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn write_kissconfig_with_threshold(root: &Path, settle: f64, threshold: u8) {
    std::fs::write(
        root.join(".kissconfig"),
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             orphan_module_enabled = false\n\
             \n\
[test]\n\
             test_coverage_threshold = {threshold}\n\
             watch_settle_seconds = {settle}\n\
             \n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n"
        ),
    )
    .unwrap();
}

#[allow(clippy::zombie_processes)]
pub fn start_watch(dir: &Path, args: &[&str]) -> WatchProc {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watch");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if dir
            .join(".kiss")
            .join("watch")
            .join("session.json")
            .is_file()
        {
            return WatchProc { child };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch session not ready");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn wait_watch_idle_cycle(_dir: &Path) {
    std::thread::sleep(Duration::from_secs(3));
}
