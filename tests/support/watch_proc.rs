use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
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

pub fn wait_watch_idle_cycle(dir: &Path) {
    let session_path = dir.join(".kiss").join("watch").join("session.json");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(socket) = session_socket_path(&session_path)
            && nudge_watcher(&socket)
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!("watch idle cycle not ready");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn session_socket_path(session_path: &Path) -> Option<String> {
    let bytes = std::fs::read(session_path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let key = "\"socket\"";
    let start = text.find(key)? + key.len();
    let rest = text.get(start..)?;
    let colon = rest.find(':')?;
    let after = rest.get(colon + 1..)?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn nudge_watcher(socket: &str) -> bool {
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(90)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let body = b"{}";
    let len = u32::try_from(body.len()).expect("nudge frame fits u32");
    if stream.write_all(&len.to_be_bytes()).is_err() || stream.write_all(body).is_err() {
        return false;
    }
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        return false;
    }
    let n = usize::try_from(u32::from_be_bytes(len_buf)).unwrap_or(0);
    if n == 0 || n > 256 * 1024 {
        return false;
    }
    let mut reply = vec![0u8; n];
    stream.read_exact(&mut reply).is_ok()
}
