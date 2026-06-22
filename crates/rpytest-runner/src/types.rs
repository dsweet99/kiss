use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedArtifact {
    pub name: String,
    pub path: PathBuf,
}

#[cfg(test)]
impl RequestedArtifact {
    pub(crate) fn witness() -> Self {
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
    pub(crate) fn witness() -> Self {
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

impl TestStatus {
    pub fn from_exit_status(status: ExitStatus) -> Self {
        if status.success() {
            Self::Passed
        } else {
            Self::Failed
        }
    }
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
    pub(crate) fn witness() -> Self {
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
    Protocol(String),
    Spawn { program: PathBuf, message: String },
    Timeout(Duration),
    WorkerPanic,
}
