//! Rust line coverage with conservative per-selector caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod cargo_runner;
mod llvm_cov_json;
mod rust_cov_cache;

#[cfg(test)]
mod cargo_runner_test;
#[cfg(test)]
mod lib_test;
#[cfg(test)]
mod rust_cov_cache_test;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use cargo_runner::{
    CargoLlvmCovRunError, CargoLlvmCovRunOutcome, CargoLlvmCovRunRequest, CargoLlvmCovRunner,
    subprocess_cargo_llvm_cov_runner,
};
use llvm_cov_json::parse_llvm_cov_json_file;
use rpytest_runner::TestStatus;
use rust_cov_cache::{
    RustCovCacheEntry, load_rust_cov_cache_entry, rust_cov_fingerprint, store_rust_cov_cache_entry,
};
use serde::{Deserialize, Serialize};

const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustLlvmCovRequest {
    pub selector: String,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub cargo: PathBuf,
    pub llvm_cov_version: String,
    pub rustc_version: String,
    pub cargo_args: Vec<String>,
    pub test_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cache_root: PathBuf,
    pub force_rerun: bool,
}

#[cfg(test)]
impl RustLlvmCovRequest {
    fn witness(root: &Path) -> Self {
        rust_cov_sample_request(root)
    }
}

pub use llvm_cov_json::RustLineCoverage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustCovCacheStatus {
    Hit,
    MissStored,
}

#[cfg(test)]
impl RustCovCacheStatus {
    fn witness_hit() -> Self {
        Self::Hit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustLlvmCovOutcome {
    pub selector: String,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub coverage: RustLineCoverage,
    pub cache_status: RustCovCacheStatus,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

#[cfg(test)]
impl RustLlvmCovOutcome {
    fn witness() -> Self {
        Self {
            selector: "smoke::passes".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage::witness(),
            cache_status: RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        }
    }
}

#[derive(Debug)]
pub enum RustLlvmCovError {
    Io(io::Error),
    Json(serde_json::Error),
    Runner(CargoLlvmCovRunError),
    InvalidRequest(String),
    MissingArtifact(PathBuf),
}

impl From<io::Error> for RustLlvmCovError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RustLlvmCovError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<CargoLlvmCovRunError> for RustLlvmCovError {
    fn from(value: CargoLlvmCovRunError) -> Self {
        Self::Runner(value)
    }
}

pub struct RustLlvmCov {
    runner: CargoLlvmCovRunner,
}

impl RustLlvmCov {
    pub fn new(runner: CargoLlvmCovRunner) -> Self {
        Self { runner }
    }

    pub fn run_or_reuse(
        &self,
        req: RustLlvmCovRequest,
    ) -> Result<RustLlvmCovOutcome, RustLlvmCovError> {
        validate_rust_cov_request(&req)?;
        fs::create_dir_all(&req.cache_root)?;
        let fingerprint = rust_cov_fingerprint(&req)?;
        if !req.force_rerun
            && let Some(entry) = load_rust_cov_cache_entry(&req.cache_root, &fingerprint)
        {
            return Ok(rust_cov_outcome_from_cache(entry));
        }

        let artifact_path = req
            .cache_root
            .join("artifacts")
            .join(format!("{fingerprint}.json"));
        if let Some(parent) = artifact_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let worker_root = req.cache_root.join("workers").join(&fingerprint);
        fs::create_dir_all(&worker_root)?;
        let run_req = build_cargo_runner_request(&req, &artifact_path, &worker_root);
        let run = self.runner.run_one(run_req)?;
        let coverage = if run.status == TestStatus::Passed {
            parse_llvm_cov_json_file(&run.artifact_path, &req.source_root)?
        } else {
            RustLineCoverage {
                files: BTreeMap::new(),
            }
        };
        let outcome = RustLlvmCovOutcome {
            selector: run.selector,
            status: run.status,
            exit_code: run.exit_code,
            duration: run.duration,
            coverage,
            cache_status: RustCovCacheStatus::MissStored,
            stdout: Some(run.stdout),
            stderr: Some(run.stderr),
        };
        store_rust_cov_cache_entry(
            &req.cache_root,
            &fingerprint,
            &RustCovCacheEntry::from(&outcome),
        )?;
        Ok(outcome)
    }
}

pub fn build_llvm_cov_argv(req: &CargoLlvmCovRunRequest) -> Vec<String> {
    let mut argv = vec![
        req.cargo.to_string_lossy().to_string(),
        "llvm-cov".to_string(),
        "test".to_string(),
        "--json".to_string(),
        "--output-path".to_string(),
        req.artifact_path.to_string_lossy().to_string(),
    ];
    argv.extend(req.cargo_args.iter().cloned());
    argv.push(req.selector.clone());
    argv.push("--".to_string());
    argv.extend(req.test_args.iter().cloned());
    argv
}

fn build_cargo_runner_request(
    req: &RustLlvmCovRequest,
    artifact_path: &Path,
    worker_root: &Path,
) -> CargoLlvmCovRunRequest {
    let mut env = req.env.clone();
    env.insert(
        "CARGO_TARGET_DIR".to_string(),
        worker_root.join("target").to_string_lossy().to_string(),
    );
    env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        worker_root
            .join("profile")
            .join("%m-%p.profraw")
            .to_string_lossy()
            .to_string(),
    );
    env.insert(
        "TMPDIR".to_string(),
        worker_root.join("tmp").to_string_lossy().to_string(),
    );
    CargoLlvmCovRunRequest {
        selector: req.selector.clone(),
        cwd: req.cwd.clone(),
        cargo: req.cargo.clone(),
        cargo_args: req.cargo_args.clone(),
        test_args: req.test_args.clone(),
        env,
        artifact_path: artifact_path.to_path_buf(),
    }
}

fn rust_cov_outcome_from_cache(entry: RustCovCacheEntry) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: entry.selector,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        cache_status: RustCovCacheStatus::Hit,
        stdout: None,
        stderr: None,
    }
}

fn validate_rust_cov_request(req: &RustLlvmCovRequest) -> Result<(), RustLlvmCovError> {
    if req.selector.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "rust test selector must not be empty".to_string(),
        ));
    }
    if req.llvm_cov_version.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "cargo llvm-cov version must be part of the cache key".to_string(),
        ));
    }
    if req.rustc_version.trim().is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "rustc version must be part of the cache key".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn rust_cov_sample_request(root: &Path) -> RustLlvmCovRequest {
    RustLlvmCovRequest {
        selector: "smoke::passes".to_string(),
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
        rustc_version: "rustc 1.88.0".to_string(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: vec!["--nocapture".to_string()],
        env: BTreeMap::new(),
        cache_root: root.join(".rust_llvm_cov_cache"),
        force_rerun: false,
    }
}
