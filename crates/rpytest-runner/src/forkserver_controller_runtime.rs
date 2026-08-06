use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::forkserver_controller::FORKSERVER_CONTROLLER;
use crate::forkserver_wire::{
    WireBootstrap, WireBootstrapResult, WireRequest, WireResponse, WireShutdown,
};
use crate::runner::validate_request;
use crate::{PytestBootstrap, PytestRunError, PytestRunOutcome, PytestRunRequest};

pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct ForkserverController {
    pub(crate) python: PathBuf,
    pub(crate) bootstrap: PytestBootstrap,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
    shutting_down: bool,
}

impl ForkserverController {
    pub(crate) fn start(python: &Path, bootstrap: &PytestBootstrap) -> Result<Self, PytestRunError> {
        let mut child = Command::new(python)
            .current_dir("/")
            .arg("-u")
            .arg("-c")
            .arg(&*FORKSERVER_CONTROLLER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| PytestRunError::Spawn {
                program: python.to_path_buf(),
                message: err.to_string(),
            })?;
        let stdin = take_stdin(&mut child)?;
        let stdout = take_stdout(&mut child)?;
        let mut controller = Self {
            python: python.to_path_buf(),
            bootstrap: bootstrap.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            shutting_down: false,
        };
        controller.bootstrap_parent(bootstrap)?;
        Ok(controller)
    }

    fn bootstrap_parent(&mut self, bootstrap: &PytestBootstrap) -> Result<(), PytestRunError> {
        let wire = WireBootstrap {
            op: "bootstrap",
            cwd: bootstrap.cwd.to_string_lossy().to_string(),
            pytest_args: bootstrap.pytest_args.clone(),
            env: bootstrap.env.clone(),
        };
        self.write_json(&wire)?;
        let response: WireBootstrapResult = self.read_json()?;
        if response.ok {
            return Ok(());
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
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

    pub(crate) fn run(
        &mut self,
        req: PytestRunRequest,
    ) -> Result<PytestRunOutcome, PytestRunError> {
        validate_request(&req)?;
        let timeout = req.timeout;
        let started = Instant::now();
        let request_id = self.next_id;
        self.next_id += 1;
        self.write_json(&WireRequest::from_request(request_id, &req))?;
        let response: WireResponse = self.read_json()?;
        outcome_from_response(response, request_id, timeout, started)
    }

    pub(crate) fn shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        if self.write_json(&WireShutdown { op: "shutdown" }).is_err() {
            force_reap(&mut self.child);
            return;
        }
        wait_or_kill(&mut self.child, SHUTDOWN_TIMEOUT);
    }

    fn write_json<T: Serialize>(&mut self, value: &T) -> Result<(), PytestRunError> {
        serde_json::to_writer(&mut self.stdin, value)
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
        self.stdin
            .flush()
            .map_err(|err| PytestRunError::Protocol(err.to_string()))
    }

    fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, PytestRunError> {
        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
        if n == 0 {
            return Err(PytestRunError::Protocol(
                "controller exited before response".to_string(),
            ));
        }
        serde_json::from_str(&line).map_err(|err| PytestRunError::Protocol(err.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn controller_pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for ForkserverController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn take_stdin(child: &mut Child) -> Result<ChildStdin, PytestRunError> {
    child
        .stdin
        .take()
        .ok_or_else(|| PytestRunError::Protocol("controller stdin unavailable".to_string()))
}

fn take_stdout(child: &mut Child) -> Result<std::process::ChildStdout, PytestRunError> {
    child
        .stdout
        .take()
        .ok_or_else(|| PytestRunError::Protocol("controller stdout unavailable".to_string()))
}

fn force_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_or_kill(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            force_reap(child);
            return;
        }
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                force_reap(child);
                return;
            }
        }
    }
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
    Ok(PytestRunOutcome {
        nodeid: response.nodeid,
        status,
        exit_code: response.exit_code,
        stdout: response.stdout,
        stderr: response.stderr,
        duration: started.elapsed(),
        artifacts: response
            .artifacts
            .into_iter()
            .map(|(name, path)| (name, PathBuf::from(path)))
            .collect(),
    })
}
