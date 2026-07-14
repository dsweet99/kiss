//! Python 3.12+ line coverage with conservative per-test caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod batch;
mod cache;
mod runtime;

#[cfg(test)]
mod batch_test;
#[cfg(test)]
mod cache_test;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cache::{RslipCacheEntry, rslip_unique_suffix};
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
        self.run_or_reuse_many_bounded(vec![req], 1)
            .into_iter()
            .next()
            .expect("one-request batch returned one result")
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
    env.insert(
        "TESTMON_DATAFILE".to_string(),
        req.cache_root
            .join("testmon")
            .join(format!("{}.testmondata", rslip_unique_suffix()))
            .to_string_lossy()
            .to_string(),
    );
    PytestRunRequest {
        nodeid: req.nodeid.clone(),
        cwd: req.cwd.clone(),
        python: req.python.clone(),
        pytest_args: req.pytest_args.clone(),
        env,
        child_preload_modules: vec![runtime::MODULE_NAME.to_string()],
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
fn fake_runner(calls: std::rc::Rc<std::cell::Cell<usize>>) -> PytestRunner {
    PytestRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        assert_eq!(
            req.child_preload_modules,
            vec![runtime::MODULE_NAME.to_string()]
        );
        assert!(req.env.contains_key("RSLIP_COVERAGE_OUT"));
        assert!(req.env.contains_key("RSLIP_SOURCE_ROOT"));
        let path = req.artifacts[0].path.clone();
        fs::write(&path, r#"{"files":{"/project/app.py":[1,3]}}"#).unwrap();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: format!("fresh stdout {}", calls.get()).into_bytes(),
            stderr: format!("fresh stderr {}", calls.get()).into_bytes(),
            duration: Duration::from_millis(7),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    })
}

#[cfg(test)]
fn python() -> PathBuf {
    std::env::var_os("PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("python"))
}

#[cfg(test)]
fn python_version(python: &Path) -> String {
    let output = std::process::Command::new(python)
        .arg("-c")
        .arg("import sys; print('.'.join(map(str, sys.version_info[:3])))")
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[cfg(test)]
mod tests;
