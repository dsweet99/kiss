//! Pytest execution boundary for tools that need per-test outcomes.
//!
//! The cold subprocess runner is intentionally small. A later forkserver runner
//! can implement the same trait without changing coverage or cache callers.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedArtifact {
    pub name: String,
    pub path: PathBuf,
}

#[cfg(test)]
impl RequestedArtifact {
    fn witness() -> Self {
        Self {
            name: "coverage".to_string(),
            path: PathBuf::from("coverage.json"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PytestRunRequest {
    pub nodeid: String,
    pub cwd: PathBuf,
    pub python: PathBuf,
    pub pytest_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub preload_modules: Vec<String>,
    pub artifacts: Vec<RequestedArtifact>,
    pub timeout: Option<Duration>,
}

#[cfg(test)]
impl PytestRunRequest {
    fn witness() -> Self {
        Self {
            nodeid: "test_sample.py::test_ok".to_string(),
            cwd: PathBuf::from("."),
            python: PathBuf::from("python"),
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::from([("A".to_string(), "B".to_string())]),
            preload_modules: vec!["preload_mod".to_string()],
            artifacts: vec![RequestedArtifact::witness()],
            timeout: Some(Duration::from_secs(1)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PytestRunOutcome {
    pub nodeid: String,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration: Duration,
    pub artifacts: BTreeMap<String, PathBuf>,
}

#[cfg(test)]
impl PytestRunOutcome {
    fn witness() -> Self {
        Self {
            nodeid: "test_sample.py::test_ok".to_string(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
            duration: Duration::from_millis(3),
            artifacts: BTreeMap::from([("coverage".to_string(), PathBuf::from("coverage.json"))]),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PytestRunError {
    InvalidRequest(String),
    Spawn { program: PathBuf, message: String },
    TimeoutUnsupported(Duration),
}

pub struct PytestRunner {
    run_one: Box<dyn Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError>>,
}

impl PytestRunner {
    pub fn from_fn<F>(run_one: F) -> Self
    where
        F: Fn(PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> + 'static,
    {
        Self {
            run_one: Box::new(run_one),
        }
    }

    pub fn run_one(&self, req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
        (self.run_one)(req)
    }

    pub fn run_many(
        &self,
        reqs: Vec<PytestRunRequest>,
    ) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
        reqs.into_iter().map(|req| self.run_one(req)).collect()
    }
}

pub fn subprocess_pytest_runner() -> PytestRunner {
    PytestRunner::from_fn(|req| {
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
        if let Some(timeout) = req.timeout {
            return Err(PytestRunError::TimeoutUnsupported(timeout));
        }

        let started = Instant::now();
        let mut cmd = Command::new(&req.python);
        cmd.current_dir(&req.cwd);
        cmd.envs(&req.env);
        cmd.arg("-c").arg(PYTEST_MAIN);
        cmd.arg(req.preload_modules.join("\x1f"));
        cmd.arg(&req.nodeid);
        cmd.args(&req.pytest_args);

        let output = cmd.output().map_err(|err| PytestRunError::Spawn {
            program: req.python.clone(),
            message: err.to_string(),
        })?;
        let exit_code = output.status.code();
        let status = if output.status.success() {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        };
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
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn python() -> PathBuf {
        std::env::var_os("PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("python"))
    }

    #[test]
    fn run_many_preserves_request_order() {
        let runner = PytestRunner::from_fn(|req| {
            Ok(PytestRunOutcome {
                nodeid: req.nodeid,
                status: TestStatus::Passed,
                exit_code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                duration: Duration::ZERO,
                artifacts: BTreeMap::new(),
            })
        });

        let cwd = PathBuf::from(".");
        let req = |nodeid: &str| PytestRunRequest {
            nodeid: nodeid.to_string(),
            cwd: cwd.clone(),
            python: PathBuf::from("python"),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: None,
        };
        let got = runner.run_many(vec![req("a.py::test_a"), req("b.py::test_b")]);
        assert_eq!(got[0].as_ref().unwrap().nodeid, "a.py::test_a");
        assert_eq!(got[1].as_ref().unwrap().nodeid, "b.py::test_b");
    }

    #[test]
    fn api_structs_expose_expected_fields() {
        let artifact = RequestedArtifact::witness();
        assert_eq!(artifact.name, "coverage");
        assert_eq!(artifact.path, PathBuf::from("coverage.json"));
        assert_eq!(TestStatus::Passed, TestStatus::Passed);

        let req = PytestRunRequest::witness();
        assert_eq!(req.nodeid, "test_sample.py::test_ok");
        assert_eq!(req.pytest_args, vec!["-q"]);
        assert_eq!(req.env["A"], "B");
        assert_eq!(req.preload_modules, vec!["preload_mod"]);
        assert_eq!(req.artifacts[0].name, "coverage");
        assert_eq!(req.timeout, Some(Duration::from_secs(1)));

        let outcome = PytestRunOutcome::witness();
        assert_eq!(outcome.status, TestStatus::Failed);
        assert_eq!(outcome.stdout, b"out");
        assert_eq!(outcome.stderr, b"err");
        assert_eq!(
            outcome.artifacts["coverage"],
            PathBuf::from("coverage.json")
        );
    }

    #[test]
    fn subprocess_runner_runs_one_pytest_node() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "def test_ok():\n    assert 2 + 2 == 4\n\ndef test_other():\n    assert False\n",
        )
        .unwrap();

        let outcome = subprocess_pytest_runner()
            .run_one(PytestRunRequest {
                nodeid: "test_sample.py::test_ok".to_string(),
                cwd: tmp.path().to_path_buf(),
                python: python(),
                pytest_args: vec!["-q".to_string()],
                env: BTreeMap::new(),
                preload_modules: Vec::new(),
                artifacts: Vec::new(),
                timeout: None,
            })
            .unwrap();

        assert_eq!(outcome.status, TestStatus::Passed);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&outcome.stdout).contains("1 passed"));
    }

    #[test]
    fn subprocess_runner_imports_preload_before_pytest() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("preload_flag.py"),
            "import os\nopen(os.environ['FLAG_PATH'], 'w').write('loaded')\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        let flag_path = tmp.path().join("flag.txt");
        let mut env = BTreeMap::new();
        env.insert(
            "PYTHONPATH".to_string(),
            tmp.path().to_string_lossy().to_string(),
        );
        env.insert(
            "FLAG_PATH".to_string(),
            flag_path.to_string_lossy().to_string(),
        );

        let outcome = subprocess_pytest_runner()
            .run_one(PytestRunRequest {
                nodeid: "test_sample.py::test_ok".to_string(),
                cwd: tmp.path().to_path_buf(),
                python: python(),
                pytest_args: vec!["-q".to_string()],
                env,
                preload_modules: vec!["preload_flag".to_string()],
                artifacts: Vec::new(),
                timeout: None,
            })
            .unwrap();

        assert_eq!(outcome.status, TestStatus::Passed);
        assert_eq!(fs::read_to_string(flag_path).unwrap(), "loaded");
    }

    #[test]
    fn timeout_is_explicitly_unsupported_for_cold_runner() {
        let err = subprocess_pytest_runner()
            .run_one(PytestRunRequest {
                nodeid: "test_x.py::test_x".to_string(),
                cwd: PathBuf::from("."),
                python: PathBuf::from("python"),
                pytest_args: Vec::new(),
                env: BTreeMap::new(),
                preload_modules: Vec::new(),
                artifacts: Vec::new(),
                timeout: Some(Duration::from_millis(1)),
            })
            .unwrap_err();

        assert_eq!(
            err,
            PytestRunError::TimeoutUnsupported(Duration::from_millis(1))
        );
    }

    #[test]
    fn invalid_request_rejects_missing_required_fields() {
        let valid = PytestRunRequest {
            nodeid: "test_x.py::test_x".to_string(),
            cwd: PathBuf::from("."),
            python: PathBuf::from("python"),
            pytest_args: Vec::new(),
            env: BTreeMap::new(),
            preload_modules: Vec::new(),
            artifacts: Vec::new(),
            timeout: None,
        };

        let mut missing_nodeid = valid.clone();
        missing_nodeid.nodeid.clear();
        assert!(matches!(
            subprocess_pytest_runner().run_one(missing_nodeid),
            Err(PytestRunError::InvalidRequest(message)) if message.contains("node id")
        ));

        let mut missing_python = valid.clone();
        missing_python.python = PathBuf::new();
        assert!(matches!(
            subprocess_pytest_runner().run_one(missing_python),
            Err(PytestRunError::InvalidRequest(message)) if message.contains("python")
        ));

        let mut missing_cwd = valid;
        missing_cwd.cwd = PathBuf::new();
        assert!(matches!(
            subprocess_pytest_runner().run_one(missing_cwd),
            Err(PytestRunError::InvalidRequest(message)) if message.contains("cwd")
        ));
    }
}
