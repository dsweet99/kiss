use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::runner::validate_request;
use crate::{PytestRunError, PytestRunOutcome, PytestRunRequest, TestStatus};

#[derive(Clone, Copy, Debug, Default)]
pub struct ForkserverPytestRunner;

impl ForkserverPytestRunner {
    pub fn new() -> Self {
        Self
    }

    pub fn run_one(&self, req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
        let python = req.python.clone();
        let mut controller = ForkserverController::start(&python)?;
        controller.run(req)
    }

    pub fn run_many(
        &self,
        reqs: Vec<PytestRunRequest>,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        let max_jobs = if reqs.is_empty() { 1 } else { reqs.len() };
        self.run_many_bounded(reqs, max_jobs)
    }

    pub fn run_many_bounded(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        assert!(max_jobs > 0, "max_jobs must be greater than zero");
        let len = reqs.len();
        let mut out = Vec::new();
        out.resize_with(len, || Err(PytestRunError::WorkerPanic));
        if len == 0 {
            return out;
        }

        let queue = Arc::new(Mutex::new(reqs.into_iter().enumerate().collect()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..max_jobs.min(len) {
            spawn_forkserver_worker(Arc::clone(&queue), tx.clone());
        }
        drop(tx);

        for (index, result) in rx {
            out[index] = result;
        }
        out
    }
}

pub fn forkserver_pytest_runner() -> crate::PytestRunner {
    crate::PytestRunner::forkserver()
}

pub(crate) struct ForkserverController {
    pub(crate) python: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl ForkserverController {
    pub(crate) fn start(python: &Path) -> Result<Self, PytestRunError> {
        let mut child = Command::new(python)
            .current_dir("/")
            .arg("-u")
            .arg("-c")
            .arg(FORKSERVER_CONTROLLER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| PytestRunError::Spawn {
                program: python.to_path_buf(),
                message: err.to_string(),
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PytestRunError::Protocol("controller stdin unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PytestRunError::Protocol("controller stdout unavailable".to_string()))?;
        Ok(Self {
            python: python.to_path_buf(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
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
        let wire_req = WireRequest::from_request(request_id, &req);
        serde_json::to_writer(&mut self.stdin, &wire_req)
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;
        self.stdin
            .flush()
            .map_err(|err| PytestRunError::Protocol(err.to_string()))?;

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
        let response: WireResponse =
            serde_json::from_str(&line).map_err(|err| PytestRunError::Protocol(err.to_string()))?;
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
}

impl Drop for ForkserverController {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn spawn_forkserver_worker(
    queue: Arc<Mutex<VecDeque<(usize, PytestRunRequest)>>>,
    tx: mpsc::Sender<(usize, Result<PytestRunOutcome, PytestRunError>)>,
) {
    thread::spawn(move || {
        let mut controller: Option<ForkserverController> = None;
        loop {
            let Some((index, req)) = queue.lock().unwrap().pop_front() else {
                break;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_reused_controller(&mut controller, req)
            }))
            .unwrap_or(Err(PytestRunError::WorkerPanic));
            let _ = tx.send((index, result));
        }
    });
}

pub(crate) fn run_with_reused_controller(
    controller: &mut Option<ForkserverController>,
    req: PytestRunRequest,
) -> Result<PytestRunOutcome, PytestRunError> {
    let needs_controller = controller
        .as_ref()
        .is_none_or(|existing| existing.python != req.python);
    if needs_controller {
        *controller = Some(ForkserverController::start(&req.python)?);
    }
    controller
        .as_mut()
        .expect("controller initialized")
        .run(req)
}

#[derive(Serialize)]
pub(crate) struct WireRequest {
    pub(crate) id: u64,
    pub(crate) nodeid: String,
    pub(crate) cwd: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) preload_modules: Vec<String>,
    pub(crate) artifacts: Vec<WireArtifact>,
    pub(crate) timeout_ms: Option<u64>,
}

impl WireRequest {
    pub(crate) fn from_request(id: u64, req: &PytestRunRequest) -> Self {
        Self {
            id,
            nodeid: req.nodeid.clone(),
            cwd: req.cwd.to_string_lossy().to_string(),
            pytest_args: req.pytest_args.clone(),
            env: req.env.clone(),
            preload_modules: req.preload_modules.clone(),
            artifacts: req
                .artifacts
                .iter()
                .map(|artifact| WireArtifact {
                    name: artifact.name.clone(),
                    path: artifact.path.to_string_lossy().to_string(),
                })
                .collect(),
            timeout_ms: req.timeout.map(duration_millis_u64),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct WireArtifact {
    pub(crate) name: String,
    pub(crate) path: String,
}

#[cfg(test)]
impl WireArtifact {
    pub(crate) fn witness() -> Self {
        Self {
            name: "coverage".to_string(),
            path: "coverage.json".to_string(),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct WireResponse {
    pub(crate) id: u64,
    pub(crate) nodeid: String,
    pub(crate) status: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) artifacts: BTreeMap<String, String>,
    pub(crate) timeout: bool,
    pub(crate) error: Option<String>,
}

#[cfg(test)]
impl WireResponse {
    pub(crate) fn witness(status: &str) -> Self {
        Self {
            id: 0,
            nodeid: "test_sample.py::test_ok".to_string(),
            status: status.to_string(),
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            artifacts: BTreeMap::new(),
            timeout: false,
            error: None,
        }
    }
}

impl WireResponse {
    pub(crate) fn test_status(&self) -> Result<TestStatus, PytestRunError> {
        match self.status.as_str() {
            "passed" => Ok(TestStatus::Passed),
            "failed" => Ok(TestStatus::Failed),
            other => Err(PytestRunError::Protocol(format!(
                "unknown test status from controller: {other}"
            ))),
        }
    }
}

pub(crate) fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

const FORKSERVER_CONTROLLER: &str = r#"
import importlib
import json
import os
import signal
import sys
import tempfile
import traceback

import pytest


def _respond(obj):
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def _read_file(path):
    try:
        with open(path, "rb") as f:
            return list(f.read())
    except FileNotFoundError:
        return []


def _run_child(req, stdout_path, stderr_path):
    try:
        os.chdir(req["cwd"])
        os.environ.update(req.get("env", {}))
        stdout_fd = os.open(stdout_path, os.O_WRONLY | os.O_TRUNC)
        stderr_fd = os.open(stderr_path, os.O_WRONLY | os.O_TRUNC)
        os.dup2(stdout_fd, 1)
        os.dup2(stderr_fd, 2)
        os.close(stdout_fd)
        os.close(stderr_fd)

        timeout_ms = req.get("timeout_ms")
        if timeout_ms is not None:
            def _timeout(_signum, _frame):
                print("pytest timed out", file=sys.stderr, flush=True)
                os._exit(124)
            signal.signal(signal.SIGALRM, _timeout)
            signal.setitimer(signal.ITIMER_REAL, max(timeout_ms / 1000.0, 0.001))

        for module_name in req.get("preload_modules", []):
            importlib.import_module(module_name)

        args = [req["nodeid"]] + list(req.get("pytest_args", []))
        os._exit(int(pytest.main(args)))
    except BaseException:
        traceback.print_exc()
        os._exit(1)


def _handle(req):
    stdout_fd, stdout_path = tempfile.mkstemp(prefix="rpytest-forkserver-out-")
    stderr_fd, stderr_path = tempfile.mkstemp(prefix="rpytest-forkserver-err-")
    os.close(stdout_fd)
    os.close(stderr_fd)
    try:
        pid = os.fork()
        if pid == 0:
            _run_child(req, stdout_path, stderr_path)
        _pid, status = os.waitpid(pid, 0)
        if os.WIFEXITED(status):
            exit_code = os.WEXITSTATUS(status)
        elif os.WIFSIGNALED(status):
            exit_code = 128 + os.WTERMSIG(status)
        else:
            exit_code = 1
        timed_out = req.get("timeout_ms") is not None and exit_code == 124
        artifacts = {a["name"]: a["path"] for a in req.get("artifacts", [])}
        return {
            "id": req["id"],
            "nodeid": req.get("nodeid", ""),
            "status": "passed" if exit_code == 0 else "failed",
            "exit_code": exit_code,
            "stdout": _read_file(stdout_path),
            "stderr": _read_file(stderr_path),
            "artifacts": artifacts,
            "timeout": timed_out,
            "error": None,
        }
    finally:
        for path in (stdout_path, stderr_path):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass


for line in sys.stdin:
    try:
        request = json.loads(line)
        _respond(_handle(request))
    except BaseException as exc:
        _respond({
            "id": request.get("id", 0) if "request" in locals() else 0,
            "nodeid": request.get("nodeid", "") if "request" in locals() else "",
            "status": "failed",
            "exit_code": None,
            "stdout": [],
            "stderr": [],
            "artifacts": {},
            "timeout": False,
            "error": "controller protocol error: " + repr(exc),
        })
"#;
