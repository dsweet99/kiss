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
             \n\
[test]\n\
             test_coverage_threshold = {threshold}\n\
             watch_settle_seconds = {settle}\n\
             \n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n\
             [python]\n\
             [rust]\n"
        ),
    )
    .unwrap();
}

impl WatchProc {
    pub fn still_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

#[allow(clippy::zombie_processes)]
pub fn spawn_watch(dir: &Path, args: &[&str]) -> WatchProc {
    let child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watch");
    WatchProc { child }
}

#[allow(clippy::zombie_processes)]
pub fn start_watch(dir: &Path, args: &[&str]) -> WatchProc {
    let mut watch = spawn_watch(dir, args);
    wait_watch_session(dir, &mut watch);
    watch
}

#[allow(clippy::zombie_processes)]
pub fn start_watch_logged(dir: &Path, args: &[&str], log_path: &Path) -> WatchProc {
    let stdout = std::fs::File::create(log_path).expect("create watcher test log");
    let stderr = stdout.try_clone().expect("clone watcher test log");
    let child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn logged watch");
    let mut watch = WatchProc { child };
    wait_watch_session(dir, &mut watch);
    watch
}

pub fn wait_watch_session(dir: &Path, watch: &mut WatchProc) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if dir
            .join(".kiss")
            .join("watch")
            .join("session.json")
            .is_file()
        {
            return;
        }
        if !watch.still_running() {
            panic!("watch exited before session was ready");
        }
        if Instant::now() >= deadline {
            panic!("watch session not ready");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn wait_watch_idle_cycle(_dir: &Path) {
    std::thread::sleep(Duration::from_secs(3));
}
