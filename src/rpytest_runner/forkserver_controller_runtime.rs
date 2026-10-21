use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod proc;
#[cfg(unix)]
mod zygote;

pub use proc::cancel_active_forkservers;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::rpytest_runner::forkserver_controller::FORKSERVER_CONTROLLER;
use crate::rpytest_runner::forkserver_wire::{
    WireBootstrap, WireBootstrapResult, WireCollect, WireCollectResult, WireModuleRequest,
    WireModuleResponse, WireRequest, WireResponse, WireShutdown,
};
use crate::rpytest_runner::runner::validate_request;
use crate::rpytest_runner::{PytestBootstrap, PytestRunError, PytestRunOutcome, PytestRunRequest};

#[cfg(test)]
pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(400);

pub fn enable_zygote() {
    zygote_flag().store(true, std::sync::atomic::Ordering::Relaxed);
}

fn zygote_enabled() -> bool {
    zygote_flag().load(std::sync::atomic::Ordering::Relaxed)
}

fn zygote_flag() -> &'static std::sync::atomic::AtomicBool {
    static ENABLED: std::sync::OnceLock<std::sync::atomic::AtomicBool> = std::sync::OnceLock::new();
    ENABLED.get_or_init(|| std::sync::atomic::AtomicBool::new(false))
}

pub(crate) struct ForkserverCollect {
    pub(crate) nodeids: Vec<String>,
    pub(crate) observed_workspace: Vec<String>,
    pub(crate) unsupported_external: bool,
    pub(crate) python_version: String,
    pub(crate) pytest_version: String,
    pub(crate) executable: String,
    pub(crate) pytest_file: String,
}

pub(crate) struct ForkserverController {
    pub(crate) python: PathBuf,
    pub(crate) bootstrap: PytestBootstrap,
    pid: u32,
    owned: Option<Child>,
    stdin: Box<dyn Write + Send>,
    stdout: BufReader<Box<dyn Read + Send>>,
    next_id: u64,
    shutting_down: bool,
    scratch: Option<PathBuf>,
}

impl ForkserverController {
    pub(crate) fn start(
        python: &Path,
        bootstrap: &PytestBootstrap,
    ) -> Result<Self, PytestRunError> {
        let scratch = proc::create_scratch_dir(&bootstrap.cwd);
        #[cfg(unix)]
        if zygote_enabled()
            && let Some(controller) = zygote::start_from_zygote(python, bootstrap, scratch.clone())
        {
            return controller;
        }
        start_owned(python, bootstrap, scratch)
    }

    fn bootstrap_parent(&mut self, bootstrap: &PytestBootstrap) -> Result<(), PytestRunError> {
        let wire = WireBootstrap {
            op: "bootstrap",
            cwd: bootstrap.cwd.to_string_lossy().to_string(),
            pytest_args: bootstrap.pytest_args.clone(),
            env: bootstrap.env.clone(),
            scratch: self
                .scratch
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        write_json(&mut self.stdin, &wire)?;
        let response: WireBootstrapResult = read_json(&mut self.stdout)?;
        if response.ok {
            return Ok(());
        }
        self.kill_controller();
        self.shutting_down = true;
        let mut message = response
            .error
            .unwrap_or_else(|| "bootstrap failed".to_string());
        if !response.stderr.is_empty() {
            message.push('\n');
            message.push_str(&String::from_utf8_lossy(&response.stderr));
        }
        Err(PytestRunError::Protocol(message))
    }

    pub(crate) fn collect_paths(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<ForkserverCollect, PytestRunError> {
        write_json(
            &mut self.stdin,
            &WireCollect {
                op: "collect",
                paths: paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
            },
        )?;
        let response: WireCollectResult = read_json(&mut self.stdout)?;
        if !response.ok {
            return Err(PytestRunError::Protocol(
                response
                    .error
                    .unwrap_or_else(|| "pytest collection failed".to_string()),
            ));
        }
        Ok(ForkserverCollect {
            nodeids: response.nodeids,
            observed_workspace: response.observed_workspace,
            unsupported_external: response.unsupported_external,
            python_version: response.python_version,
            pytest_version: response.pytest_version,
            executable: response.executable,
            pytest_file: response.pytest_file,
        })
    }

    pub(crate) fn run(
        &mut self,
        req: PytestRunRequest,
    ) -> Result<PytestRunOutcome, PytestRunError> {
        validate_request(&req)?;
        let timeout = req.timeout;
        let started = Instant::now();
        let request_id = self.next_id;
        self.next_id += 1;
        write_json(&mut self.stdin, &WireRequest::from_request(request_id, &req))?;
        let response: WireResponse = read_json(&mut self.stdout)?;
        outcome_from_response(response, request_id, timeout, started)
    }

    pub(crate) fn run_module_once(
        &mut self,
        reqs: &[PytestRunRequest],
        on_result: &mut dyn FnMut(usize, Result<PytestRunOutcome, PytestRunError>),
    ) -> Result<Vec<Result<PytestRunOutcome, PytestRunError>>, PytestRunError> {
        for req in reqs {
            validate_request(req)?;
        }
        let started = Instant::now();
        let mut tests = Vec::with_capacity(reqs.len());
        let mut ids = Vec::with_capacity(reqs.len());
        for req in reqs {
            let request_id = self.next_id;
            self.next_id += 1;
            ids.push((request_id, req.timeout));
            tests.push(WireRequest::from_request(request_id, req));
        }
        let first = reqs.first().expect("module batch is non-empty");
        write_json(&mut self.stdin, &WireModuleRequest {
            op: "run_module",
            cwd: first.cwd.to_string_lossy().to_string(),
            pytest_args: first.pytest_args.clone(),
            child_preload_modules: first.child_preload_modules.clone(),
            tests,
        })?;
        let response = loop {
            let mut response: WireModuleResponse = read_json(&mut self.stdout)?;
            let Some(progress) = response.progress.take() else {
                break response;
            };
            if let Some(position) = ids.iter().position(|(id, _)| *id == progress.id) {
                let (request_id, timeout) = ids[position];
                on_result(
                    position,
                    outcome_from_response(progress, request_id, timeout, started),
                );
            }
        };
        if let Some(error) = response.error {
            return Err(PytestRunError::Protocol(error));
        }
        if response.results.len() != reqs.len() {
            return Err(PytestRunError::Protocol(format!(
                "module batch returned {} results for {} requests",
                response.results.len(),
                reqs.len()
            )));
        }
        let mut outcomes = Vec::with_capacity(reqs.len());
        for (response, (request_id, timeout)) in response.results.into_iter().zip(ids) {
            outcomes.push(outcome_from_response(
                response, request_id, timeout, started,
            ));
        }
        Ok(outcomes)
    }

    pub(crate) fn shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        let _ = write_json(&mut self.stdin, &WireShutdown { op: "shutdown" });
        self.kill_controller();
    }

    #[cfg(test)]
    pub(crate) fn shutdown_graceful(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        if write_json(&mut self.stdin, &WireShutdown { op: "shutdown" }).is_err() {
            self.kill_controller();
            return;
        }
        self.wait_for_exit(SHUTDOWN_TIMEOUT);
    }

    #[cfg(test)]
    pub(crate) fn controller_pid(&self) -> u32 {
        self.pid
    }

    fn kill_controller(&mut self) {
        if let Some(child) = self.owned.as_mut() {
            proc::force_reap(child);
            return;
        }
        proc::signal_pid(self.pid);
    }

    #[cfg(test)]
    fn wait_for_exit(&mut self, timeout: Duration) {
        if let Some(child) = self.owned.as_mut() {
            proc::wait_or_kill(child, timeout);
            return;
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !proc::process_alive(self.pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        proc::signal_pid(self.pid);
    }
}

impl Drop for ForkserverController {
    fn drop(&mut self) {
        proc::unregister_active_forkserver(self.pid);
        self.shutdown();
        if let Some(dir) = &self.scratch {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

const SCRATCH_ENV: &str = "KISS_FORKSERVER_SCRATCH";

fn start_owned(
    python: &Path,
    bootstrap: &PytestBootstrap,
    scratch: Option<PathBuf>,
) -> Result<ForkserverController, PytestRunError> {
    let mut command = Command::new(python);
    command
        .current_dir("/")
        .arg("-u")
        .arg("-c")
        .arg(&*FORKSERVER_CONTROLLER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(dir) = &scratch {
        command.env(SCRATCH_ENV, dir);
    }
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "linux")]
    proc::kill_on_parent_exit(&mut command);
    let mut child = command.spawn().map_err(|err| PytestRunError::Spawn {
        program: python.to_path_buf(),
        message: err.to_string(),
    })?;
    let pid = child.id();
    let stdin = proc::take_stdin(&mut child)?;
    let stdout = proc::take_stdout(&mut child)?;
    let mut controller = ForkserverController {
        python: python.to_path_buf(),
        bootstrap: bootstrap.clone(),
        pid,
        owned: Some(child),
        stdin: Box::new(stdin),
        stdout: BufReader::new(Box::new(stdout)),
        next_id: 0,
        shutting_down: false,
        scratch,
    };
    proc::register_active_forkserver(pid);
    controller.bootstrap_parent(bootstrap)?;
    Ok(controller)
}

fn write_json<T: Serialize>(
    stdin: &mut (dyn Write + Send),
    value: &T,
) -> Result<(), PytestRunError> {
    serde_json::to_writer(&mut *stdin, value)
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    stdin
        .write_all(b"\n")
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    stdin
        .flush()
        .map_err(|err| PytestRunError::Protocol(err.to_string()))
}

fn read_json<T: DeserializeOwned>(
    stdout: &mut BufReader<Box<dyn Read + Send>>,
) -> Result<T, PytestRunError> {
    let mut line = String::new();
    let n = stdout
        .read_line(&mut line)
        .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
    if n == 0 {
        return Err(PytestRunError::Protocol(
            "controller exited before response".to_string(),
        ));
    }
    serde_json::from_str(&line).map_err(|err| PytestRunError::Protocol(err.to_string()))
}

fn outcome_from_response(
    response: WireResponse,
    request_id: u64,
    timeout: Option<Duration>,
    started: Instant,
) -> Result<PytestRunOutcome, PytestRunError> {
    if response.id != request_id {
        return Err(PytestRunError::Protocol(format!(
            "controller response id {} did not match request id {request_id}",
            response.id
        )));
    }
    if let Some(error) = response.error {
        return Err(PytestRunError::Protocol(error));
    }
    if response.timeout {
        return Err(PytestRunError::Timeout(
            timeout.unwrap_or(Duration::from_secs(0)),
        ));
    }
    let status = response.test_status()?;
    let duration = response
        .test_duration_ms
        .map(Duration::from_millis)
        .unwrap_or_else(|| started.elapsed());
    Ok(PytestRunOutcome {
        nodeid: response.nodeid,
        status,
        exit_code: response.exit_code,
        stdout: response.stdout,
        stderr: response.stderr,
        duration,
        artifacts: response
            .artifacts
            .into_iter()
            .map(|(name, path)| (name, PathBuf::from(path)))
            .collect(),
    })
}
