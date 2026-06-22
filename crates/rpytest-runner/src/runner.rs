use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use crate::{PytestRunError, PytestRunOutcome, PytestRunRequest, TestStatus};

pub struct PytestRunner {
    run_one: Rc<dyn Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError>>,
    run_many: Box<dyn Fn(Vec<PytestRunRequest>) -> Vec<Result<PytestRunOutcome, PytestRunError>>>,
}

impl PytestRunner {
    pub fn from_fn<F>(run_one: F) -> Self
    where
        F: Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> + 'static,
    {
        let run_one: Rc<dyn Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError>> =
            Rc::new(run_one);
        let run_many_one = Rc::clone(&run_one);
        Self {
            run_one,
            run_many: Box::new(move |reqs| reqs.into_iter().map(|req| run_many_one(req)).collect()),
        }
    }

    pub fn subprocess() -> Self {
        Self {
            run_one: Rc::new(|req| SubprocessPytestRunner::new().run_one(req)),
            run_many: Box::new(|reqs| SubprocessPytestRunner::new().run_many(reqs)),
        }
    }

    pub fn run_one(&self, req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
        (self.run_one)(req)
    }

    pub fn run_many(
        &self,
        reqs: Vec<PytestRunRequest>,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        (self.run_many)(reqs)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SubprocessPytestRunner;

impl SubprocessPytestRunner {
    pub fn new() -> Self {
        Self
    }
}

impl SubprocessPytestRunner {
    pub fn run_one(&self, req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
        if req.nodeid.trim().is_empty() {
            return Err(PytestRunError::InvalidRequest(
                "pytest node id must not be empty".to_string(),
            ));
        }
        if req.cwd.as_os_str().is_empty() {
            return Err(PytestRunError::InvalidRequest(
                "pytest cwd must not be empty".to_string(),
            ));
        }
        if req.python.as_os_str().is_empty() {
            return Err(PytestRunError::InvalidRequest(
                "python executable must not be empty".to_string(),
            ));
        }
        let started = Instant::now();
        let mut cmd = Command::new(&req.python);
        cmd.current_dir(&req.cwd);
        cmd.envs(&req.env);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.arg("-c").arg(PYTEST_MAIN);
        cmd.arg(req.preload_modules.join("\x1f"));
        cmd.arg(&req.nodeid);
        cmd.args(&req.pytest_args);

        let output = run_command(cmd, &req.python, req.timeout)?;
        let exit_code = output.status.code();
        let status = TestStatus::from_exit_status(output.status);
        let artifacts = req
            .artifacts
            .into_iter()
            .map(|artifact| (artifact.name, artifact.path))
            .collect();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status,
            exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            duration: started.elapsed(),
            artifacts,
        })
    }

    pub fn run_many(
        &self,
        reqs: Vec<PytestRunRequest>,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        let mut handles = Vec::with_capacity(reqs.len());
        for (index, req) in reqs.into_iter().enumerate() {
            handles.push(thread::spawn(move || {
                (index, SubprocessPytestRunner.run_one(req))
            }));
        }

        let mut out = Vec::new();
        out.resize_with(handles.len(), || Err(PytestRunError::WorkerPanic));
        for handle in handles {
            match handle.join() {
                Ok((index, result)) => out[index] = result,
                Err(_) => out.push(Err(PytestRunError::WorkerPanic)),
            }
        }
        out
    }
}

pub fn subprocess_pytest_runner() -> PytestRunner {
    PytestRunner::subprocess()
}

fn run_command(
    mut cmd: Command,
    program: &Path,
    timeout: Option<Duration>,
) -> Result<Output, PytestRunError> {
    let mut child = cmd.spawn().map_err(|err| PytestRunError::Spawn {
        program: program.to_path_buf(),
        message: err.to_string(),
    })?;
    let Some(timeout) = timeout else {
        return child.wait_with_output().map_err(|err| PytestRunError::Spawn {
            program: program.to_path_buf(),
            message: err.to_string(),
        });
    };
    let started = Instant::now();
    loop {
        if started.elapsed() >= timeout {
            child.kill().map_err(|err| PytestRunError::Spawn {
                program: program.to_path_buf(),
                message: err.to_string(),
            })?;
            child.wait().map_err(|err| PytestRunError::Spawn {
                program: program.to_path_buf(),
                message: err.to_string(),
            })?;
            return Err(PytestRunError::Timeout(timeout));
        }
        match child.try_wait().map_err(|err| PytestRunError::Spawn {
            program: program.to_path_buf(),
            message: err.to_string(),
        })? {
            Some(_) => {
                return child.wait_with_output().map_err(|err| PytestRunError::Spawn {
                    program: program.to_path_buf(),
                    message: err.to_string(),
                });
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
}

const PYTEST_MAIN: &str = r#"
import importlib
import sys

preloads = sys.argv[1].split("\x1f") if sys.argv[1] else []
for module_name in preloads:
    importlib.import_module(module_name)

import pytest

raise SystemExit(pytest.main(sys.argv[2:]))
"#;
