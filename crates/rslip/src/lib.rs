//! Python 3.12+ line coverage with conservative per-test caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod cache;
mod runtime;

#[cfg(test)]
mod cache_test;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cache::{
    RslipCacheEntry, load_rslip_cache_entry, rslip_cache_fingerprint, store_rslip_cache_entry,
};
use rpytest_runner::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner, RequestedArtifact, TestStatus,
};
use serde::{Deserialize, Serialize};

pub const CACHE_SCHEMA_VERSION: &str = "rslip-cache-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RslipRequest {
    pub nodeid: String,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub python: PathBuf,
    pub python_version: String,
    pub pytest_version: String,
    pub pytest_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cache_root: PathBuf,
    pub force_rerun: bool,
}

#[cfg(test)]
impl RslipRequest {
    fn witness(root: &Path) -> Self {
        rslip_sample_request(root)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCoverage {
    pub files: BTreeMap<String, BTreeSet<u32>>,
}

#[cfg(test)]
impl LineCoverage {
    fn witness() -> Self {
        Self {
            files: BTreeMap::from([("app.py".to_string(), BTreeSet::from([1, 2]))]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheStatus {
    Hit,
    MissStored,
}

#[cfg(test)]
impl CacheStatus {
    fn witness_hit() -> Self {
        Self::Hit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RslipOutcome {
    pub nodeid: String,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub coverage: LineCoverage,
    pub cache_status: CacheStatus,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

#[cfg(test)]
impl RslipOutcome {
    fn witness() -> Self {
        Self {
            nodeid: "test_sample.py::test_ok".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: LineCoverage::witness(),
            cache_status: CacheStatus::Hit,
            stdout: None,
            stderr: None,
        }
    }
}

#[derive(Debug)]
pub enum RslipError {
    Io(io::Error),
    Json(serde_json::Error),
    Runner(PytestRunError),
    MissingArtifact(String),
    InvalidRequest(String),
}

impl From<io::Error> for RslipError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RslipError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<PytestRunError> for RslipError {
    fn from(value: PytestRunError) -> Self {
        Self::Runner(value)
    }
}

pub struct Rslip {
    runner: PytestRunner,
}

impl Rslip {
    pub fn new(runner: PytestRunner) -> Self {
        Self { runner }
    }

    pub fn run_or_reuse(&self, req: RslipRequest) -> Result<RslipOutcome, RslipError> {
        validate_rslip_request(&req)?;
        fs::create_dir_all(&req.cache_root)?;
        let fingerprint = rslip_cache_fingerprint(&req)?;
        if !req.force_rerun
            && let Some(entry) = load_rslip_cache_entry(&req.cache_root, &fingerprint)
        {
            return Ok(rslip_outcome_from_cache(entry));
        }

        let run_dir = req.cache_root.join("runtime");
        fs::create_dir_all(&run_dir)?;
        let runtime_path = run_dir.join(format!("{}.py", runtime::MODULE_NAME));
        fs::write(&runtime_path, runtime::PYTHON_RUNTIME)?;
        let artifact_path = req
            .cache_root
            .join("artifacts")
            .join(format!("{fingerprint}.json"));
        if let Some(parent) = artifact_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let runner_req = build_pytest_runner_request(&req, &run_dir, &artifact_path);
        let outcome = self.runner.run_one(runner_req)?;
        let coverage = rslip_coverage_from_outcome(&outcome)?;
        let rslip_outcome = RslipOutcome {
            nodeid: outcome.nodeid,
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage,
            cache_status: CacheStatus::MissStored,
            stdout: Some(outcome.stdout),
            stderr: Some(outcome.stderr),
        };
        store_rslip_cache_entry(
            &req.cache_root,
            &fingerprint,
            &RslipCacheEntry::from(&rslip_outcome),
        )?;
        Ok(rslip_outcome)
    }
}

fn rslip_outcome_from_cache(entry: RslipCacheEntry) -> RslipOutcome {
    RslipOutcome {
        nodeid: entry.nodeid,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        cache_status: CacheStatus::Hit,
        stdout: None,
        stderr: None,
    }
}

fn validate_rslip_request(req: &RslipRequest) -> Result<(), RslipError> {
    if req.nodeid.trim().is_empty() {
        return Err(RslipError::InvalidRequest(
            "pytest node id must not be empty".to_string(),
        ));
    }
    if req.pytest_version.trim().is_empty() {
        return Err(RslipError::InvalidRequest(
            "pytest version must be part of the cache key".to_string(),
        ));
    }
    if req.python_version.trim().is_empty() {
        return Err(RslipError::InvalidRequest(
            "python version must be part of the cache key".to_string(),
        ));
    }
    Ok(())
}

fn build_pytest_runner_request(
    req: &RslipRequest,
    runtime_dir: &Path,
    artifact_path: &Path,
) -> PytestRunRequest {
    let mut env = req.env.clone();
    let python_path = match env.get("PYTHONPATH") {
        Some(existing) if !existing.is_empty() => {
            format!("{}:{}", runtime_dir.display(), existing)
        }
        _ => runtime_dir.to_string_lossy().to_string(),
    };
    env.insert("PYTHONPATH".to_string(), python_path);
    env.insert(
        "RSLIP_COVERAGE_OUT".to_string(),
        artifact_path.to_string_lossy().to_string(),
    );
    env.insert(
        "RSLIP_SOURCE_ROOT".to_string(),
        req.source_root.to_string_lossy().to_string(),
    );
    PytestRunRequest {
        nodeid: req.nodeid.clone(),
        cwd: req.cwd.clone(),
        python: req.python.clone(),
        pytest_args: req.pytest_args.clone(),
        env,
        preload_modules: vec![runtime::MODULE_NAME.to_string()],
        artifacts: vec![RequestedArtifact {
            name: runtime::COVERAGE_ARTIFACT.to_string(),
            path: artifact_path.to_path_buf(),
        }],
        timeout: None,
    }
}

fn rslip_coverage_from_outcome(outcome: &PytestRunOutcome) -> Result<LineCoverage, RslipError> {
    let path = outcome
        .artifacts
        .get(runtime::COVERAGE_ARTIFACT)
        .ok_or_else(|| RslipError::MissingArtifact(runtime::COVERAGE_ARTIFACT.to_string()))?;
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
fn rslip_sample_request(root: &Path) -> RslipRequest {
    RslipRequest {
        nodeid: "test_sample.py::test_ok".to_string(),
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        python: PathBuf::from("python"),
        python_version: "3.12.0".to_string(),
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: root.join(".rslip_cache"),
        force_rerun: false,
    }
}

#[cfg(test)]
mod tests;
