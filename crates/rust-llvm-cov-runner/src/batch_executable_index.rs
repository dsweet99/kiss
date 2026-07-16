use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use crate::batch_events::selector_matches_test;
use crate::batch_executor_finish::{digest_test_binary, test_binary_id_for_path};
use crate::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::batch_plan::{RustCoverageBatchPlan, RustCoverageBatchRequest};
use crate::batch_result::RustCoverageBatchCounters;
use crate::batch_run::{self, CurrentRunCleanup, FreshBatchRunScope};
use crate::batch_shim::load_target_runner_list_metadata;
use crate::{RustLlvmCovError, RustTestBinaryIdentity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustTestExecutableIndex {
    pub selector_binary_ids: BTreeMap<String, Vec<String>>,
    pub test_binaries: Vec<RustTestBinaryIdentity>,
    pub counters: RustCoverageBatchCounters,
}

pub fn build_rust_test_executable_index(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    _identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
) -> Result<RustTestExecutableIndex, RustLlvmCovError> {
    crate::batch_platform::ensure_batch_platform_supported()?;
    let scope =
        FreshBatchRunScope::begin_with_layout(&req.cache_root, plan, CurrentRunCleanup::default())
            .map_err(RustLlvmCovError::from)?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let outcome = (|| {
        crate::batch_runner_resolve::write_runner_map(
            &plan.runner_map_path,
            &req.delegated_runners,
        )?;
        crate::batch_plan_publish::publish_generated_nextest_config(plan)?;
        let mut list_plan = plan.clone();
        select_no_tests(&mut list_plan);
        let run = batch_run::default_batch_subprocess_runner()
            .run(&req.cwd, &list_plan)
            .map_err(RustLlvmCovError::from)?;
        if run.exit_code != Some(0) {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "nextest list build failed: stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            )));
        }
        let build_target_baseline_bytes = batch_run::publish_successful_build_identity(
            req,
            tools,
            plan,
            build_identity.previous_baseline_bytes,
        )?;
        let index = executable_index_from_list_metadata(req, plan)?;
        Ok(RustTestExecutableIndex {
            selector_binary_ids: index.selector_binary_ids,
            test_binaries: index.test_binaries,
            counters: RustCoverageBatchCounters {
                build_invocations: 1,
                build_target_baseline_bytes,
                process_residual_count: run.process_residual_count,
                ..Default::default()
            },
        })
    })();
    match outcome {
        Ok(index) => scope.finish(Ok(index)),
        Err(err) => scope.finish(Err(err)),
    }
}

fn select_no_tests(plan: &mut RustCoverageBatchPlan) {
    if !plan.argv.iter().any(|arg| arg == "--") {
        plan.argv.push("--".to_string());
    }
    plan.argv.push("--skip".to_string());
    plan.argv.push(String::new());
}

fn executable_index_from_list_metadata(
    req: &RustCoverageBatchRequest,
    plan: &RustCoverageBatchPlan,
) -> Result<RustTestExecutableIndex, RustLlvmCovError> {
    let exact = req.test_args.iter().any(|arg| arg == "--exact");
    let list_metadata = load_target_runner_list_metadata(&plan.target_runner_output_dir)
        .map_err(RustLlvmCovError::Io)?;
    if list_metadata.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "no Rust test executable metadata produced by nextest list phase".into(),
        ));
    }
    let mut binary_by_id = BTreeMap::new();
    let mut selector_binary_ids = BTreeMap::<String, BTreeSet<String>>::new();
    for item in list_metadata {
        let executable = item
            .argv
            .first()
            .ok_or_else(|| RustLlvmCovError::InvalidRequest("missing list executable".into()))?;
        let path = std::path::Path::new(executable);
        let id = test_binary_id_for_path(path);
        let digest = digest_test_binary(path)?;
        binary_by_id.insert(
            id.clone(),
            RustTestBinaryIdentity {
                id: id.clone(),
                executable: executable.clone(),
                digest,
            },
        );
        let test_names = list_test_names_from_executable(path, &id)?;
        for selector in &req.logical_selectors {
            if test_names
                .iter()
                .any(|test_name| selector_matches_test(test_name, selector, exact))
            {
                selector_binary_ids
                    .entry(selector.clone())
                    .or_default()
                    .insert(id.clone());
            }
        }
    }
    let selector_binary_ids = selector_binary_ids
        .into_iter()
        .map(|(selector, ids)| (selector, ids.into_iter().collect()))
        .collect();
    Ok(RustTestExecutableIndex {
        selector_binary_ids,
        test_binaries: binary_by_id.into_values().collect(),
        counters: RustCoverageBatchCounters::default(),
    })
}

fn list_test_names_from_executable(
    path: &std::path::Path,
    binary_id: &str,
) -> Result<Vec<String>, RustLlvmCovError> {
    let output = Command::new(path)
        .arg("--list")
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "test binary list failed for {}: stdout:\n{}\nstderr:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| test_name_from_list_line(line, binary_id))
        .collect())
}

fn test_name_from_list_line(line: &str, binary_id: &str) -> Option<String> {
    let test_name = line.strip_suffix(": test")?;
    Some(format!("{binary_id}${test_name}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn list_plan_skips_every_test_without_no_run() {
        let mut plan = crate::RustCoverageBatchPlan::witness();

        super::select_no_tests(&mut plan);

        assert!(!plan.argv.iter().any(|arg| arg == "--no-run"));
        assert!(plan.argv.ends_with(&["--skip".to_string(), String::new()]));
    }

    #[test]
    fn parses_libtest_list_lines_with_canonical_binary_id() {
        assert_eq!(
            super::test_name_from_list_line("module::case: test", "/tmp/bin"),
            Some("/tmp/bin$module::case".to_string())
        );
        assert_eq!(
            super::test_name_from_list_line("module::bench: bench", "/tmp/bin"),
            None
        );
    }
}
