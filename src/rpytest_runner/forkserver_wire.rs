use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::rpytest_runner::{PytestRunError, PytestRunRequest, TestStatus};

#[derive(Serialize)]
pub(crate) struct WireBootstrap {
    pub(crate) op: &'static str,
    pub(crate) cwd: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub(crate) struct WireBootstrapResult {
    pub(crate) ok: bool,
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) stderr: Vec<u8>,
}

#[derive(Serialize)]
pub(crate) struct WireShutdown {
    pub(crate) op: &'static str,
}

#[derive(Serialize)]
pub(crate) struct WireRequest {
    pub(crate) id: u64,
    pub(crate) nodeid: String,
    pub(crate) cwd: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) child_preload_modules: Vec<String>,
    pub(crate) artifacts: Vec<WireArtifact>,
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct WireModuleRequest {
    pub(crate) op: &'static str,
    pub(crate) cwd: String,
    pub(crate) pytest_args: Vec<String>,
    pub(crate) child_preload_modules: Vec<String>,
    pub(crate) tests: Vec<WireRequest>,
}

#[derive(Deserialize)]
pub(crate) struct WireModuleResponse {
    #[serde(default)]
    pub(crate) results: Vec<WireResponse>,
    pub(crate) error: Option<String>,
}

impl WireRequest {
    pub(crate) fn from_request(id: u64, req: &PytestRunRequest) -> Self {
        Self {
            id,
            nodeid: req.nodeid.clone(),
            cwd: req.cwd.to_string_lossy().to_string(),
            pytest_args: req.pytest_args.clone(),
            env: req.env.clone(),
            child_preload_modules: req.child_preload_modules.clone(),
            artifacts: req
                .artifacts
                .iter()
                .map(|artifact| WireArtifact {
                    name: artifact.name.clone(),
                    path: artifact.path.to_string_lossy().to_string(),
                })
                .collect(),
            timeout_ms: req
                .timeout
                .map(crate::rpytest_runner::forkserver::duration_millis_u64),
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
    #[serde(default)]
    pub(crate) test_duration_ms: Option<u64>,
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
            test_duration_ms: None,
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
