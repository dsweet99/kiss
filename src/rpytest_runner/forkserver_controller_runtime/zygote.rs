use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use crate::rpytest_runner::forkserver_controller::FORKSERVER_CONTROLLER;
use crate::rpytest_runner::{PytestBootstrap, PytestRunError};

use super::proc;
use super::ForkserverController;

pub(super) fn start_from_zygote(
    python: &Path,
    bootstrap: &PytestBootstrap,
    scratch: Option<PathBuf>,
) -> Option<Result<ForkserverController, PytestRunError>> {
    if zygote_disabled() {
        return None;
    }
    let sock_name = match zygote_socket_name(python) {
        Ok(name) => name,
        Err(_) => {
            disable_zygote();
            return None;
        }
    };
    match connect_forked_controller(python, bootstrap, scratch, &sock_name) {
        Ok(controller) => Some(Ok(controller)),
        Err(_) => {
            disable_zygote();
            None
        }
    }
}

fn connect_forked_controller(
    python: &Path,
    bootstrap: &PytestBootstrap,
    scratch: Option<PathBuf>,
    sock_name: &Path,
) -> Result<ForkserverController, PytestRunError> {
    let stream = UnixStream::connect(sock_name).map_err(|err| PytestRunError::Spawn {
        program: python.to_path_buf(),
        message: err.to_string(),
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    let reader_stream = stream
        .try_clone()
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    let mut stdout = BufReader::new(Box::new(reader_stream) as Box<dyn Read + Send>);
    let mut line = String::new();
    let n = stdout
        .read_line(&mut line)
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    stream
        .set_read_timeout(None)
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    if n == 0 {
        return Err(PytestRunError::Protocol(
            "forked controller exited before ready".to_string(),
        ));
    }
    let ready: WireControllerReady = serde_json::from_str(line.trim()).map_err(|err| {
        PytestRunError::Protocol(format!("forked controller ready: {err}; line={line}"))
    })?;
    if ready.op != "ready" {
        return Err(PytestRunError::Protocol(format!(
            "forked controller ready op {}",
            ready.op
        )));
    }
    let mut controller = ForkserverController {
        python: python.to_path_buf(),
        bootstrap: bootstrap.clone(),
        pid: ready.pid,
        owned: None,
        stdin: Box::new(stream),
        stdout,
        next_id: 0,
        shutting_down: false,
        scratch,
    };
    proc::register_active_forkserver(ready.pid);
    controller.bootstrap_parent(bootstrap)?;
    Ok(controller)
}

#[derive(serde::Deserialize)]
struct WireControllerReady {
    op: String,
    pid: u32,
}

struct ZygoteProc {
    python: PathBuf,
    sock_path: PathBuf,
    child: Child,
}

fn zygote_pool() -> &'static Mutex<Vec<ZygoteProc>> {
    static POOL: OnceLock<Mutex<Vec<ZygoteProc>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

fn zygote_disabled() -> bool {
    zygote_broken().load(std::sync::atomic::Ordering::Relaxed)
}

fn disable_zygote() {
    zygote_broken().store(true, std::sync::atomic::Ordering::Relaxed);
    let mut pool = zygote_pool()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for zygote in pool.drain(..) {
        let mut child = zygote.child;
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(zygote.sock_path);
    }
}

fn zygote_broken() -> &'static std::sync::atomic::AtomicBool {
    static BROKEN: OnceLock<std::sync::atomic::AtomicBool> = OnceLock::new();
    BROKEN.get_or_init(|| std::sync::atomic::AtomicBool::new(false))
}

struct ZygoteShutdown;

impl Drop for ZygoteShutdown {
    fn drop(&mut self) {
        disable_zygote();
    }
}

fn ensure_zygote_shutdown_hook() {
    static HOOK: OnceLock<ZygoteShutdown> = OnceLock::new();
    let _ = HOOK.get_or_init(|| ZygoteShutdown);
}

fn zygote_socket_name(python: &Path) -> Result<PathBuf, PytestRunError> {
    ensure_zygote_shutdown_hook();
    let mut pool = zygote_pool()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut index = 0;
    while index < pool.len() {
        match pool[index].child.try_wait() {
            Ok(None) => {
                if pool[index].python == python {
                    return Ok(pool[index].sock_path.clone());
                }
                index += 1;
            }
            _ => {
                let dead = pool.remove(index);
                let _ = std::fs::remove_file(dead.sock_path);
            }
        }
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sock_path =
        std::env::temp_dir().join(format!("kiss-rpytest-{}-{n}.sock", std::process::id()));
    let child = spawn_zygote(python, &sock_path)?;
    pool.push(ZygoteProc {
        python: python.to_path_buf(),
        sock_path: sock_path.clone(),
        child,
    });
    Ok(sock_path)
}

fn spawn_zygote(python: &Path, sock_path: &Path) -> Result<Child, PytestRunError> {
    let mut command = Command::new(python);
    command
        .current_dir("/")
        .arg("-u")
        .arg("-c")
        .arg(&*FORKSERVER_CONTROLLER)
        .env("KISS_FORKSERVER_MODE", "zygote")
        .env("KISS_FORKSERVER_ZYGOTE_SOCK", sock_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    proc::kill_on_parent_exit(&mut command);
    let mut child = command.spawn().map_err(|err| PytestRunError::Spawn {
        program: python.to_path_buf(),
        message: err.to_string(),
    })?;
    let mut stdout = BufReader::new(proc::take_stdout(&mut child)?);
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    if !line.contains("zygote_ready") {
        let mut err_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err_buf);
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(sock_path);
        return Err(PytestRunError::Protocol(format!(
            "zygote failed: {line} {err_buf}"
        )));
    }
    drop(child.stderr.take());
    Ok(child)
}
