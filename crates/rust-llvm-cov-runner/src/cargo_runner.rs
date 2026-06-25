use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rpytest_runner::TestStatus;

use crate::build_llvm_cov_argv;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoLlvmCovRunRequest {
    pub selector: String,
    pub cwd: PathBuf,
    pub cargo: PathBuf,
    pub cargo_args: Vec<String>,
    pub test_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub artifact_path: PathBuf,
}

impl CargoLlvmCovRunRequest {
    pub fn new(
        selector: impl Into<String>,
        cwd: PathBuf,
        cargo: PathBuf,
        artifact_path: PathBuf,
    ) -> Self {
        Self {
            selector: selector.into(),
            cwd,
            cargo,
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
            artifact_path,
        }
    }
}

#[cfg(test)]
impl CargoLlvmCovRunRequest {
    pub(super) fn witness(cargo: PathBuf, cwd: PathBuf, artifact_path: PathBuf) -> Self {
        let mut req = Self::new("smoke_sub", cwd, cargo, artifact_path);
        req.cargo_args = vec!["--workspace".to_string()];
        req.test_args = vec!["--exact".to_string()];
        req
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoLlvmCovRunOutcome {
    pub selector: String,
    pub status: TestStatus,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub artifact_path: PathBuf,
}

#[cfg(test)]
impl CargoLlvmCovRunOutcome {
    pub(super) fn witness() -> Self {
        Self {
            selector: "smoke_sub".to_string(),
            status: TestStatus::Failed,
            exit_code: Some(101),
            duration: Duration::from_millis(4),
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
            artifact_path: PathBuf::from("coverage.json"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CargoLlvmCovRunError {
    InvalidRequest(String),
    Spawn { program: PathBuf, message: String },
}

type RunnerFn =
    dyn Fn(CargoLlvmCovRunRequest) -> Result<CargoLlvmCovRunOutcome, CargoLlvmCovRunError>;

#[derive(Clone)]
pub struct CargoLlvmCovRunner {
    run: Arc<RunnerFn>,
}

impl CargoLlvmCovRunner {
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn(CargoLlvmCovRunRequest) -> Result<CargoLlvmCovRunOutcome, CargoLlvmCovRunError>
            + 'static,
    {
        Self { run: Arc::new(f) }
    }

    pub fn run_one(
        &self,
        req: CargoLlvmCovRunRequest,
    ) -> Result<CargoLlvmCovRunOutcome, CargoLlvmCovRunError> {
        if req.selector.trim().is_empty() {
            return Err(CargoLlvmCovRunError::InvalidRequest(
                "rust test selector must not be empty".to_string(),
            ));
        }
        (self.run)(req)
    }
}

pub fn subprocess_cargo_llvm_cov_runner() -> CargoLlvmCovRunner {
    CargoLlvmCovRunner::from_fn(run_subprocess)
}

pub(crate) fn run_subprocess(
    req: CargoLlvmCovRunRequest,
) -> Result<CargoLlvmCovRunOutcome, CargoLlvmCovRunError> {
    if let Some(parent) = req.artifact_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CargoLlvmCovRunError::Spawn {
            program: req.cargo.clone(),
            message: e.to_string(),
        })?;
    }
    if let Some(profile) = req.env.get("LLVM_PROFILE_FILE").map(PathBuf::from)
        && let Some(parent) = profile.parent()
    {
        std::fs::create_dir_all(parent).map_err(|e| CargoLlvmCovRunError::Spawn {
            program: req.cargo.clone(),
            message: e.to_string(),
        })?;
    }
    if let Some(tmp) = req.env.get("TMPDIR") {
        std::fs::create_dir_all(tmp).map_err(|e| CargoLlvmCovRunError::Spawn {
            program: req.cargo.clone(),
            message: e.to_string(),
        })?;
    }
    let argv = build_llvm_cov_argv(&req);
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&req.cwd)
        .envs(&req.env)
        .stdin(Stdio::null());
    let started = Instant::now();
    let output = command.output().map_err(|e| CargoLlvmCovRunError::Spawn {
        program: req.cargo.clone(),
        message: e.to_string(),
    })?;
    Ok(CargoLlvmCovRunOutcome {
        selector: req.selector,
        status: TestStatus::from_exit_status(output.status),
        exit_code: output.status.code(),
        duration: started.elapsed(),
        stdout: output.stdout,
        stderr: output.stderr,
        artifact_path: req.artifact_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(cargo: PathBuf) -> CargoLlvmCovRunRequest {
        let tmp = tempfile::tempdir().unwrap();
        CargoLlvmCovRunRequest::new(
            "smoke_sub",
            tmp.path().to_path_buf(),
            cargo,
            tmp.path().join("coverage.json"),
        )
    }

    #[test]
    fn runner_rejects_empty_selector_before_spawning() {
        let runner = CargoLlvmCovRunner::from_fn(|_| {
            panic!("runner closure should not be called for invalid selector")
        });
        let mut req = request(PathBuf::from("cargo"));
        req.selector.clear();

        let err = runner.run_one(req).unwrap_err();

        assert!(matches!(
            err,
            CargoLlvmCovRunError::InvalidRequest(message) if message.contains("selector")
        ));
    }

    #[test]
    fn subprocess_runner_reports_spawn_error() {
        let runner = subprocess_cargo_llvm_cov_runner();
        let req = request(PathBuf::from("/definitely/not/a/cargo"));

        let err = runner.run_one(req).unwrap_err();

        assert!(matches!(err, CargoLlvmCovRunError::Spawn { .. }));
    }

    #[test]
    fn outcome_type_preserves_process_result_fields() {
        let outcome = CargoLlvmCovRunOutcome {
            selector: "smoke_sub".to_string(),
            status: TestStatus::Failed,
            exit_code: Some(101),
            duration: Duration::from_millis(4),
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
            artifact_path: PathBuf::from("coverage.json"),
        };

        assert_eq!(outcome.selector, "smoke_sub");
        assert_eq!(outcome.status, TestStatus::Failed);
        assert_eq!(outcome.exit_code, Some(101));
        assert_eq!(outcome.stdout, b"out");
        assert_eq!(outcome.stderr, b"err");
    }

    #[test]
    fn request_constructor_sets_empty_args_and_env_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let req = CargoLlvmCovRunRequest::new(
            "smoke_sub",
            tmp.path().to_path_buf(),
            PathBuf::from("cargo"),
            tmp.path().join("coverage.json"),
        );

        assert_eq!(req.selector, "smoke_sub");
        assert_eq!(req.cargo, PathBuf::from("cargo"));
        assert!(req.cargo_args.is_empty());
        assert!(req.test_args.is_empty());
        assert!(req.env.is_empty());
    }
}
