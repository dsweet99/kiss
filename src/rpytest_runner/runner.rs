use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::rpytest_runner::forkserver::ForkserverPytestRunner;
use crate::rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest, TestStatus};

type PytestRunResult = Result<PytestRunOutcome, PytestRunError>;
type RunOneFn = dyn Fn(PytestRunRequest) -> PytestRunResult;
type RunManyFn = dyn Fn(Vec<PytestRunRequest>) -> Vec<PytestRunResult>;
type RunManyBoundedFn =
    dyn Fn(Vec<PytestRunRequest>, usize, &mut dyn FnMut(usize, PytestRunResult));

pub struct PytestRunner {
    run_one: Rc<RunOneFn>,
    run_many: Box<RunManyFn>,
    run_many_bounded: Box<RunManyBoundedFn>,
}

pub(crate) fn collect_bounded_results(
    reqs: Vec<PytestRunRequest>,
    max_jobs: usize,
    run: impl FnOnce(
        Vec<PytestRunRequest>,
        usize,
        &mut dyn FnMut(usize, Result<PytestRunOutcome, PytestRunError>),
    ),
) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
    let len = reqs.len();
    let mut out = Vec::new();
    out.resize_with(len, || Err(PytestRunError::WorkerPanic));
    run(reqs, max_jobs, &mut |index, result| {
        out[index] = result;
    });
    out
}

impl PytestRunner {
    pub fn from_fn<F>(run_one: F) -> Self
    where
        F: Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> + 'static,
    {
        let run_one: Rc<RunOneFn> = Rc::new(run_one);
        let run_many_one = Rc::clone(&run_one);
        let run_many_bounded_one = Rc::clone(&run_one);
        Self {
            run_one,
            run_many: Box::new(move |reqs| reqs.into_iter().map(|req| run_many_one(req)).collect()),
            run_many_bounded: Box::new(move |reqs, max_jobs, on_complete| {
                assert!(max_jobs > 0, "max_jobs must be greater than zero");
                for (index, req) in reqs.into_iter().enumerate() {
                    on_complete(index, run_many_bounded_one(req));
                }
            }),
        }
    }

    pub fn from_bounded_fn<F>(run_many_bounded: F) -> Self
    where
        F: Fn(Vec<PytestRunRequest>, usize) -> Vec<PytestRunResult> + 'static,
    {
        let run_many_bounded: Rc<dyn Fn(Vec<PytestRunRequest>, usize) -> Vec<PytestRunResult>> =
            Rc::new(run_many_bounded);
        let run_one_bounded = Rc::clone(&run_many_bounded);
        let run_many_default = Rc::clone(&run_many_bounded);
        let run_many_bounded_stream = Rc::clone(&run_many_bounded);
        Self {
            run_one: Rc::new(move |req| {
                run_one_bounded(vec![req], 1)
                    .into_iter()
                    .next()
                    .unwrap_or(Err(PytestRunError::WorkerPanic))
            }),
            run_many: Box::new(move |reqs| {
                let max_jobs = reqs.len().max(1);
                run_many_default(reqs, max_jobs)
            }),
            run_many_bounded: Box::new(move |reqs, max_jobs, on_complete| {
                assert!(max_jobs > 0, "max_jobs must be greater than zero");
                for (index, result) in run_many_bounded_stream(reqs, max_jobs)
                    .into_iter()
                    .enumerate()
                {
                    on_complete(index, result);
                }
            }),
        }
    }

    pub fn from_streaming_bounded_fn<F>(run_many_bounded: F) -> Self
    where
        F: Fn(Vec<PytestRunRequest>, usize, &mut dyn FnMut(usize, PytestRunResult)) + 'static,
    {
        let run_many_bounded: Rc<RunManyBoundedFn> = Rc::new(run_many_bounded);
        let run_one_bounded = Rc::clone(&run_many_bounded);
        let run_many_default = Rc::clone(&run_many_bounded);
        let run_many_bounded_stream = Rc::clone(&run_many_bounded);
        Self {
            run_one: Rc::new(move |req| {
                let mut result = Err(PytestRunError::WorkerPanic);
                run_one_bounded(vec![req], 1, &mut |_, completed| {
                    result = completed;
                });
                result
            }),
            run_many: Box::new(move |reqs| {
                let max_jobs = reqs.len().max(1);
                collect_bounded_results(reqs, max_jobs, |reqs, max_jobs, on_complete| {
                    run_many_default(reqs, max_jobs, on_complete);
                })
            }),
            run_many_bounded: Box::new(move |reqs, max_jobs, on_complete| {
                assert!(max_jobs > 0, "max_jobs must be greater than zero");
                run_many_bounded_stream(reqs, max_jobs, on_complete);
            }),
        }
    }

    pub fn subprocess() -> Self {
        Self {
            run_one: Rc::new(|req| SubprocessPytestRunner::new().run_one(req)),
            run_many: Box::new(|reqs| SubprocessPytestRunner::new().run_many(reqs)),
            run_many_bounded: Box::new(|reqs, max_jobs, on_complete| {
                SubprocessPytestRunner::new().run_many_bounded_with_on_complete(
                    reqs,
                    max_jobs,
                    on_complete,
                );
            }),
        }
    }

    pub fn forkserver() -> Self {
        Self {
            run_one: Rc::new(|req| ForkserverPytestRunner::new().run_one(req)),
            run_many: Box::new(|reqs| ForkserverPytestRunner::new().run_many(reqs)),
            run_many_bounded: Box::new(|reqs, max_jobs, on_complete| {
                ForkserverPytestRunner::new().run_many_bounded_with_on_complete(
                    reqs,
                    max_jobs,
                    on_complete,
                );
            }),
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

    pub fn run_many_bounded(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        collect_bounded_results(reqs, max_jobs, |reqs, max_jobs, on_complete| {
            self.run_many_bounded_with_on_complete(reqs, max_jobs, on_complete);
        })
    }

    pub fn run_many_bounded_with_on_complete(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
        on_complete: &mut dyn FnMut(usize, PytestRunResult),
    ) {
        (self.run_many_bounded)(reqs, max_jobs, on_complete);
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
        validate_request(&req)?;
        let started = Instant::now();
        let mut cmd = Command::new(&req.python);
        cmd.current_dir(&req.cwd);

        cmd.env_remove("PYTEST_ADDOPTS");
        cmd.env_remove("PYTEST_DISABLE_PLUGIN_AUTOLOAD");
        cmd.envs(&req.env);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.arg("-c").arg(PYTEST_MAIN);
        cmd.arg(req.child_preload_modules.join("\x1f"));
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
        let max_jobs = reqs.len().max(1);
        self.run_many_bounded(reqs, max_jobs)
    }

    pub fn run_many_bounded(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        collect_bounded_results(reqs, max_jobs, run_subprocess_bounded_streaming)
    }

    pub fn run_many_bounded_with_on_complete(
        &self,
        reqs: Vec<PytestRunRequest>,
        max_jobs: usize,
        mut on_complete: impl FnMut(usize, Result<PytestRunOutcome, PytestRunError>),
    ) {
        assert!(max_jobs > 0, "max_jobs must be greater than zero");
        let len = reqs.len();
        if len == 0 {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let mut indexed_reqs = reqs.into_iter().enumerate();
        let mut running = 0usize;
        for _ in 0..max_jobs.min(len) {
            if let Some((index, req)) = indexed_reqs.next() {
                spawn_subprocess_job(index, req, tx.clone());
                running += 1;
            }
        }

        while running > 0 {
            let Ok((index, result)) = rx.recv() else {
                break;
            };
            running -= 1;
            on_complete(index, result);
            if let Some((next_index, next_req)) = indexed_reqs.next() {
                spawn_subprocess_job(next_index, next_req, tx.clone());
                running += 1;
            }
        }
    }
}

fn run_subprocess_bounded_streaming(
    reqs: Vec<PytestRunRequest>,
    max_jobs: usize,
    on_complete: &mut dyn FnMut(usize, Result<PytestRunOutcome, PytestRunError>),
) {
    SubprocessPytestRunner.run_many_bounded_with_on_complete(reqs, max_jobs, on_complete);
}

pub fn subprocess_pytest_runner() -> PytestRunner {
    PytestRunner::subprocess()
}

pub(crate) fn validate_request(req: &PytestRunRequest) -> Result<(), PytestRunError> {
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
    Ok(())
}

pub(crate) fn run_command(
    mut cmd: Command,
    program: &Path,
    timeout: Option<Duration>,
) -> Result<Output, PytestRunError> {
    let mut child = cmd.spawn().map_err(|err| PytestRunError::Spawn {
        program: program.to_path_buf(),
        message: err.to_string(),
    })?;
    let Some(timeout) = timeout else {
        return child
            .wait_with_output()
            .map_err(|err| PytestRunError::Spawn {
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
                return child
                    .wait_with_output()
                    .map_err(|err| PytestRunError::Spawn {
                        program: program.to_path_buf(),
                        message: err.to_string(),
                    });
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
}

pub(crate) fn spawn_subprocess_job(
    index: usize,
    req: PytestRunRequest,
    tx: mpsc::Sender<(usize, Result<PytestRunOutcome, PytestRunError>)>,
) {
    thread::spawn(move || {
        let result = std::panic::catch_unwind(|| SubprocessPytestRunner.run_one(req))
            .unwrap_or(Err(PytestRunError::WorkerPanic));
        let _ = tx.send((index, result));
    });
}

const PYTEST_MAIN: &str = r#"
import importlib
import os
import sys

os.environ.pop("PYTEST_ADDOPTS", None)
os.environ["PYTEST_DISABLE_PLUGIN_AUTOLOAD"] = "1"

preloads = sys.argv[1].split("\x1f") if sys.argv[1] else []
for module_name in preloads:
    importlib.import_module(module_name)

import pytest

class _ClearAutoloadAfterConfigure:
    def pytest_configure(self, config):
        # Nested pytest (shell'd from tests) needs autoload for pytest.ini addopts.
        os.environ.pop("PYTEST_DISABLE_PLUGIN_AUTOLOAD", None)

# Clear ini addopts while autoload is disabled (matches collector / forkserver
# bootstrap). Otherwise pytest.ini flags like --random-order fail as unknown.
args = ["-o", "addopts=", "--import-mode=importlib"] + sys.argv[2:]
raise SystemExit(pytest.main(args, plugins=[_ClearAutoloadAfterConfigure()]))
"#;
