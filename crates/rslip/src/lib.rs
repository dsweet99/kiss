//! Python 3.12+ line coverage with conservative per-test caching.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod cache;
mod runtime;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cache::{CacheEntry, cache_fingerprint, load_cache_entry, store_cache_entry};
use rpytest_runner::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner, RequestedArtifact, TestStatus,
};
use serde::{Deserialize, Serialize};

const CACHE_SCHEMA_VERSION: &str = "rslip-cache-v1";

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
}

#[cfg(test)]
impl RslipRequest {
    fn witness(root: &Path) -> Self {
        sample_request(root)
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RslipOutcome {
    pub nodeid: String,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub coverage: LineCoverage,
    pub cache_status: CacheStatus,
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
        validate_request(&req)?;
        fs::create_dir_all(&req.cache_root)?;
        let fingerprint = cache_fingerprint(&req)?;
        if let Some(entry) = load_cache_entry(&req.cache_root, &fingerprint) {
            return Ok(outcome_from_cache(entry));
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

        let runner_req = build_runner_request(&req, &run_dir, &artifact_path);
        let outcome = self.runner.run_one(runner_req)?;
        let coverage = coverage_from_outcome(&outcome)?;
        let rslip_outcome = RslipOutcome {
            nodeid: outcome.nodeid,
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage,
            cache_status: CacheStatus::MissStored,
        };
        store_cache_entry(
            &req.cache_root,
            &fingerprint,
            &CacheEntry::from(&rslip_outcome),
        )?;
        Ok(rslip_outcome)
    }
}

fn outcome_from_cache(entry: CacheEntry) -> RslipOutcome {
    RslipOutcome {
        nodeid: entry.nodeid,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        cache_status: CacheStatus::Hit,
    }
}

fn validate_request(req: &RslipRequest) -> Result<(), RslipError> {
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

fn build_runner_request(
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

fn coverage_from_outcome(outcome: &PytestRunOutcome) -> Result<LineCoverage, RslipError> {
    let path = outcome
        .artifacts
        .get(runtime::COVERAGE_ARTIFACT)
        .ok_or_else(|| RslipError::MissingArtifact(runtime::COVERAGE_ARTIFACT.to_string()))?;
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
fn sample_request(root: &Path) -> RslipRequest {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpytest_runner::subprocess_pytest_runner;
    use std::{cell::Cell, rc::Rc};
    use std::process::Command;

    fn python() -> PathBuf {
        std::env::var_os("PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("python"))
    }

    fn python_version(python: &Path) -> String {
        let output = Command::new(python)
            .arg("-c")
            .arg("import sys; print('.'.join(map(str, sys.version_info[:3])))")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "python version command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn request_and_coverage_structs_expose_expected_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let req = RslipRequest::witness(tmp.path());
        assert_eq!(req.nodeid, "test_sample.py::test_ok");
        assert_eq!(req.python_version, "3.12.0");
        assert_eq!(req.pytest_version, "8.0.0");
        assert!(req.cache_root.ends_with(".rslip_cache"));

        let coverage = LineCoverage::witness();
        assert_eq!(coverage.files["app.py"], BTreeSet::from([1, 2]));

        let outcome = RslipOutcome::witness();
        assert_eq!(outcome.status, TestStatus::Passed);
        assert_eq!(outcome.cache_status, CacheStatus::Hit);
        assert_eq!(outcome.exit_code, Some(0));
    }

    #[test]
    fn run_or_reuse_uses_cache_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        let runner = fake_runner(Rc::clone(&calls));
        let rslip = Rslip::new(runner);
        let req = sample_request(tmp.path());

        let first = rslip.run_or_reuse(req.clone()).unwrap();
        let second = rslip.run_or_reuse(req).unwrap();

        assert_eq!(first.cache_status, CacheStatus::MissStored);
        assert_eq!(second.cache_status, CacheStatus::Hit);
        assert_eq!(
            second.coverage.files["/project/app.py"],
            BTreeSet::from([1, 3])
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn corrupt_cache_entry_is_treated_as_miss() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("test_sample.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        let req = sample_request(tmp.path());
        let fingerprint = cache_fingerprint(&req).unwrap();
        let path = cache::cache_path(&req.cache_root, &fingerprint);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{not json").unwrap();

        let calls = Rc::new(Cell::new(0));
        let runner = fake_runner(Rc::clone(&calls));
        let rslip = Rslip::new(runner);
        let outcome = rslip.run_or_reuse(req).unwrap();

        assert_eq!(outcome.cache_status, CacheStatus::MissStored);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn subprocess_run_records_executed_lines_and_reuses_cache() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("app.py"),
            "def choose(flag):\n    if flag:\n        return 1\n    return 2\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("test_app.py"),
            "from app import choose\n\n\ndef test_choose_true():\n    assert choose(True) == 1\n",
        )
        .unwrap();
        let python = python();
        let req = RslipRequest {
            nodeid: "test_app.py::test_choose_true".to_string(),
            cwd: tmp.path().to_path_buf(),
            source_root: tmp.path().to_path_buf(),
            python_version: python_version(&python),
            python,
            pytest_version: "8.0.0".to_string(),
            pytest_args: vec!["-q".to_string()],
            env: BTreeMap::new(),
            cache_root: tmp.path().join(".rslip_cache"),
        };
        let rslip = Rslip::new(subprocess_pytest_runner());

        let first = rslip.run_or_reuse(req.clone()).unwrap();
        let second = rslip.run_or_reuse(req).unwrap();
        let app_path = tmp.path().join("app.py").canonicalize().unwrap();
        let app_key = app_path.to_string_lossy().to_string();

        assert_eq!(first.status, TestStatus::Passed);
        assert_eq!(first.cache_status, CacheStatus::MissStored);
        assert_eq!(second.cache_status, CacheStatus::Hit);
        assert!(first.coverage.files[&app_key].contains(&1));
        assert!(first.coverage.files[&app_key].contains(&2));
        assert!(first.coverage.files[&app_key].contains(&3));
        assert!(!first.coverage.files[&app_key].contains(&4));
    }

    fn fake_runner(calls: Rc<Cell<usize>>) -> PytestRunner {
        PytestRunner::from_fn(move |req| {
            calls.set(calls.get() + 1);
            assert_eq!(req.preload_modules, vec![runtime::MODULE_NAME.to_string()]);
            assert!(req.env.contains_key("RSLIP_COVERAGE_OUT"));
            assert!(req.env.contains_key("RSLIP_SOURCE_ROOT"));
            let path = req.artifacts[0].path.clone();
            fs::write(&path, r#"{"files":{"/project/app.py":[1,3]}}"#).unwrap();
            Ok(PytestRunOutcome {
                nodeid: req.nodeid,
                status: TestStatus::Passed,
                exit_code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                duration: Duration::from_millis(7),
                artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
            })
        })
    }
}
