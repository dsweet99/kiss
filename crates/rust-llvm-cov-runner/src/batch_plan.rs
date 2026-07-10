use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchRequest {
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub cargo: PathBuf,
    pub cache_root: PathBuf,
    pub logical_selectors: Vec<String>,
    pub cargo_args: Vec<String>,
    pub test_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub jobs: usize,
    pub generated_config: PathBuf,
}

#[cfg(test)]
impl RustCoverageBatchRequest {
    pub(crate) fn witness() -> Self {
        Self {
            cwd: PathBuf::from("/repo"),
            source_root: PathBuf::from("/repo"),
            cargo: PathBuf::from("cargo"),
            cache_root: PathBuf::from("/repo/.kiss/rust_llvm_cov_cache"),
            logical_selectors: vec!["alpha".to_string(), "beta".to_string()],
            cargo_args: vec!["--workspace".to_string()],
            test_args: vec!["--exact".to_string()],
            env: BTreeMap::from([("KEEP_ME".to_string(), "1".to_string())]),
            jobs: 4,
            generated_config: PathBuf::from("/repo/.kiss/runs/nextest.toml"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchPlan {
    pub build_target: PathBuf,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
}

#[cfg(test)]
impl RustCoverageBatchPlan {
    pub(crate) fn witness() -> Self {
        build_rust_coverage_batch_plan(&RustCoverageBatchRequest::witness()).unwrap()
    }
}

pub fn build_rust_coverage_batch_plan(
    req: &RustCoverageBatchRequest,
) -> Result<RustCoverageBatchPlan, String> {
    validate_batch_request(req)?;
    let build_target = req.cache_root.join("build").join("target");
    let mut env = req.env.clone();
    let build_target_value = build_target.to_string_lossy().to_string();
    env.insert(
        "NEXTEST_EXPERIMENTAL_LIBTEST_JSON".to_string(),
        "1".to_string(),
    );
    env.insert("CARGO_TARGET_DIR".to_string(), build_target_value.clone());
    env.insert(
        "CARGO_LLVM_COV_TARGET_DIR".to_string(),
        build_target_value.clone(),
    );
    env.insert("CARGO_LLVM_COV_BUILD_DIR".to_string(), build_target_value);

    let jobs = req.jobs.to_string();
    let mut argv = vec![
        req.cargo.to_string_lossy().to_string(),
        "llvm-cov".to_string(),
        "nextest".to_string(),
        "--no-report".to_string(),
        "--build-jobs".to_string(),
        jobs.clone(),
        "--test-threads".to_string(),
        jobs,
        "--no-fail-fast".to_string(),
        "--retries".to_string(),
        "0".to_string(),
        "--no-tests".to_string(),
        "pass".to_string(),
        "--cargo-message-format".to_string(),
        "json".to_string(),
        "--message-format".to_string(),
        "libtest-json-plus".to_string(),
        "--message-format-version".to_string(),
        "0.1".to_string(),
        "--show-progress".to_string(),
        "none".to_string(),
        "--status-level".to_string(),
        "none".to_string(),
        "--final-status-level".to_string(),
        "none".to_string(),
        "--failure-output".to_string(),
        "never".to_string(),
        "--success-output".to_string(),
        "never".to_string(),
        "--user-config-file".to_string(),
        "none".to_string(),
        "--config-file".to_string(),
        req.generated_config.to_string_lossy().to_string(),
        "--profile".to_string(),
        "kiss".to_string(),
    ];
    argv.extend(req.cargo_args.iter().cloned());
    if !req.test_args.is_empty() {
        argv.push("--".to_string());
        argv.extend(req.test_args.iter().cloned());
    }

    Ok(RustCoverageBatchPlan {
        build_target,
        env,
        argv,
    })
}

fn validate_batch_request(req: &RustCoverageBatchRequest) -> Result<(), String> {
    if req.jobs == 0 {
        return Err("jobs must be greater than zero".to_string());
    }
    if req
        .logical_selectors
        .iter()
        .any(|selector| selector.is_empty())
    {
        return Err("logical selectors must not be empty".to_string());
    }
    validate_supported_rust_test_args(&req.test_args)?;
    Ok(())
}

pub fn validate_supported_rust_test_args(test_args: &[String]) -> Result<(), String> {
    let mut index = 0;
    while index < test_args.len() {
        let arg = &test_args[index];
        match arg.as_str() {
            "--exact" | "--nocapture" | "--no-capture" | "--ignored" | "--include-ignored" => {
                index += 1;
            }
            "--skip" => {
                let Some(pattern) = test_args.get(index + 1) else {
                    return Err("--skip requires a non-empty pattern".to_string());
                };
                if pattern.is_empty() {
                    return Err("--skip requires a non-empty pattern".to_string());
                }
                index += 2;
            }
            _ if arg.starts_with("--skip=") && arg.len() > "--skip=".len() => {
                index += 1;
            }
            _ => {
                return Err(format!(
                    "unsupported Rust test argument `{arg}`; supported forms are --exact, --nocapture, --no-capture, --ignored, --include-ignored, and repeated --skip <pattern>"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan};

    #[test]
    fn batch_request_witness_exercises_public_contract_in_module() {
        let req = RustCoverageBatchRequest::witness();
        let cloned = req.clone();

        assert_eq!(cloned, req);
        assert!(format!("{req:?}").contains("RustCoverageBatchRequest"));
        assert_eq!(req.logical_selectors.len(), 2);
        assert_eq!(req.jobs, 4);
        assert!(build_rust_coverage_batch_plan(&req).is_ok());
    }

    #[test]
    fn batch_plan_witness_exercises_public_contract_in_module() {
        let plan = RustCoverageBatchPlan::witness();
        let cloned = plan.clone();

        assert_eq!(cloned, plan);
        assert!(format!("{plan:?}").contains("RustCoverageBatchPlan"));
        assert_eq!(plan.argv[0], "cargo");
        assert_eq!(plan.env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"], "1");
    }
}
