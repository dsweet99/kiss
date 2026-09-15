use std::env;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::thread;
use std::time::{Duration, Instant};

pub fn wait_ready(work: &Path, ids: &[&str]) {
    for id in ids {
        wait_path(&work.join("ready").join(id), Duration::from_secs(10));
    }
}

pub fn wait_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(Instant::now() < deadline, "timeout {}", path.display());
        thread::sleep(Duration::from_millis(2));
    }
}

pub fn wait_barrier_ready(barrier: &Path, artifact: &str, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        for entry in fs::read_dir(barrier).into_iter().flatten().flatten() {
            let path = entry.path();
            if !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".ready.json"))
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if value.get("artifact").and_then(|v| v.as_str()) == Some(artifact)
                && value.get("phase").and_then(|v| v.as_str()) == Some(phase)
            {
                fs::write(barrier.join("ready_copy.json"), text.as_bytes()).unwrap();
                return;
            }
        }
        assert!(Instant::now() < deadline, "barrier timeout");
        thread::sleep(Duration::from_millis(2));
    }
}

#[allow(dead_code)]
pub fn release_barrier(barrier: &Path) {
    let ready = fs::read_to_string(barrier.join("ready_copy.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&ready).unwrap();
    let op = value["operation_id"].as_str().unwrap();
    let payload = format!(
        "{{\n  \"schema_version\": 1,\n  \"operation_id\": \"{op}\",\n  \"artifact\": \"{}\",\n  \"phase\": \"{}\"\n}}\n",
        value["artifact"].as_str().unwrap(),
        value["phase"].as_str().unwrap()
    );
    let tmp = barrier.join(format!(".{op}.release.tmp"));
    fs::write(&tmp, payload.as_bytes()).unwrap();
    fs::rename(tmp, barrier.join(format!("{op}.release.json"))).unwrap();
}

pub fn assert_ok(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub struct RaceChild {
    pid: libc::pid_t,
}

impl RaceChild {
    pub fn kill(&mut self) -> std::io::Result<()> {
        let rc = unsafe { libc::kill(self.pid, libc::SIGKILL) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(self.pid, &mut status, 0) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(ExitStatus::from_raw(status))
    }

    pub fn wait_with_output(mut self) -> std::io::Result<Output> {
        let status = self.wait()?;
        Ok(Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

pub struct SpawnFork<'a> {
    pub work: &'a Path,
    pub id: &'a str,
    pub mode: &'a str,
    pub child_env: &'a str,
    pub root_env: &'a str,
    pub mode_env: &'a str,
    pub barrier: Option<(&'a Path, &'a str)>,
    pub child_main: fn(),
}

pub fn spawn_fork(cfg: SpawnFork<'_>) -> RaceChild {
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());
    if pid == 0 {
        unsafe {
            env::set_var(cfg.child_env, cfg.id);
            env::set_var(cfg.root_env, cfg.work);
            env::set_var(cfg.mode_env, cfg.mode);
            env::remove_var("LLVM_PROFILE_FILE");
            if let Some((barrier, target)) = cfg.barrier {
                env::set_var("KISS_QA_PUBLICATION_BARRIER_DIR", barrier);
                env::set_var("KISS_QA_PUBLICATION_BARRIER_TARGET", target);
            }
        }
        let code = match std::panic::catch_unwind(cfg.child_main) {
            Ok(()) => 0,
            Err(_) => 1,
        };
        unsafe { libc::_exit(code) };
    }
    RaceChild { pid }
}

pub fn child_work_and_repo(child_env: &str, root_env: &str) -> (String, PathBuf, PathBuf) {
    let id = env::var(child_env).unwrap();
    let work = PathBuf::from(env::var_os(root_env).unwrap());
    let repo = PathBuf::from(fs::read_to_string(work.join("repo_path.txt")).unwrap());
    (id, work, repo)
}
