use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const SELECTOR_SPECIFIC_ENV: &[&str] = &["RSLIP_COVERAGE_OUT", "TESTMON_DATAFILE"];

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PytestBootstrap {
    pub cwd: PathBuf,
    pub pytest_args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl PytestBootstrap {
    pub fn new(cwd: PathBuf, pytest_args: Vec<String>, env: BTreeMap<String, String>) -> Self {
        Self {
            cwd,
            pytest_args,
            env: parent_safe_env(&env),
        }
    }
}

#[must_use]
pub fn parent_safe_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .filter(|(key, _)| !is_selector_specific_env(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn is_selector_specific_env(key: &str) -> bool {
    SELECTOR_SPECIFIC_ENV.contains(&key)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PytestRunRequest {
    pub nodeid: String,
    pub cwd: PathBuf,
    pub python: PathBuf,
    pub pytest_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub bootstrap: PytestBootstrap,
    pub child_preload_modules: Vec<String>,
    pub artifacts: Vec<RequestedArtifact>,
    pub timeout: Option<Duration>,
}

impl PytestRunRequest {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        nodeid: String,
        cwd: PathBuf,
        python: PathBuf,
        pytest_args: Vec<String>,
        env: BTreeMap<String, String>,
        child_preload_modules: Vec<String>,
        artifacts: Vec<RequestedArtifact>,
        timeout: Option<Duration>,
    ) -> Self {
        let bootstrap = PytestBootstrap::new(cwd.clone(), pytest_args.clone(), env.clone());
        Self {
            nodeid,
            cwd,
            python,
            pytest_args,
            env,
            bootstrap,
            child_preload_modules,
            artifacts,
            timeout,
        }
    }
}

#[cfg(test)]
impl PytestRunRequest {
    pub(crate) fn witness() -> Self {
        Self::from_parts(
            "test_sample.py::test_ok".to_string(),
            PathBuf::from("."),
            PathBuf::from("python"),
            vec!["-q".to_string()],
            BTreeMap::from([("A".to_string(), "B".to_string())]),
            vec!["preload_mod".to_string()],
            vec![RequestedArtifact::witness()],
            Some(Duration::from_secs(1)),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
    TimedOut,
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

impl PytestRunError {
    pub(crate) fn cloned(&self) -> Self {
        match self {
            Self::InvalidRequest(message) => Self::InvalidRequest(message.clone()),
            Self::Protocol(message) => Self::Protocol(message.clone()),
            Self::Spawn { program, message } => Self::Spawn {
                program: program.clone(),
                message: message.clone(),
            },
            Self::Timeout(timeout) => Self::Timeout(*timeout),
            Self::WorkerPanic => Self::WorkerPanic,
        }
    }
}
