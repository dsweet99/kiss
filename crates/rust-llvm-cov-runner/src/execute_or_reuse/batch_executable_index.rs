use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use crate::execute_or_reuse::batch_events::selector_matches_test;
use crate::execute_or_reuse::batch_executor_finish::{digest_test_binary, test_binary_id_for_path};
use crate::plan::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::plan::batch_plan::{RustCoverageBatchPlan, RustCoverageBatchRequest};
use crate::execute_or_reuse::batch_result::RustCoverageBatchCounters;
use crate::execute_or_reuse::batch_run::{self, CurrentRunCleanup, FreshBatchRunScope};
use crate::execute_or_reuse::batch_shim::load_target_runner_list_metadata;
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
    crate::plan::batch_platform::ensure_batch_platform_supported()?;
    let scope =
        FreshBatchRunScope::begin_with_layout(&req.cache_root, plan, CurrentRunCleanup::default())
            .map_err(RustLlvmCovError::from)?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let outcome = (|| {
        crate::plan::batch_runner_resolve::write_runner_map(
            &plan.runner_map_path,
            &req.delegated_runners,
        )?;
        crate::plan::batch_plan_publish::publish_generated_nextest_config(plan, req)?;
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
    let kiss_profraw = crate::kiss_profraw::kiss_profraw_dir(&req.source_root);
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
        let test_names = list_test_names_from_executable(path, &id, &kiss_profraw)?;
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
    kiss_profraw: &std::path::Path,
) -> Result<Vec<String>, RustLlvmCovError> {


    crate::kiss_profraw::ensure_kiss_profraw(kiss_profraw).map_err(RustLlvmCovError::Io)?;
    let mut command = Command::new(path);
    command.arg("--list");
    crate::execute_or_reuse::batch_shim_delegated::scrub_coverage_build_env(&mut command);
    command.env(
        "LLVM_PROFILE_FILE",
        crate::kiss_profraw::discard_llvm_profile_path(kiss_profraw),
    );
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RustLlvmCovError::Io)?;
    let child_pid = child.id();
    let output = child.wait_with_output().map_err(RustLlvmCovError::Io)?;
    let cleanup_err =
        crate::kiss_profraw::cleanup_kiss_profraw_for_pid(kiss_profraw, child_pid).err();
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "test binary list failed for {}: stdout:\n{}\nstderr:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if let Some(err) = cleanup_err {
        return Err(RustLlvmCovError::Io(err));
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
    use crate::execute_or_reuse::batch_shim::{BatchShimListMetadata, SHIM_LIST_SCHEMA};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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

    #[test]
    fn executable_index_rejects_missing_list_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let req = crate::RustCoverageBatchRequest::witness();
        let mut plan = crate::RustCoverageBatchPlan::witness();
        plan.target_runner_output_dir = tmp.path().join("missing");

        let err = super::executable_index_from_list_metadata(&req, &plan).unwrap_err();

        assert!(format!("{err:?}").contains("no Rust test executable metadata produced"));
    }

    #[test]
    fn executable_index_rejects_list_metadata_without_argv() {
        let tmp = tempfile::tempdir().unwrap();
        let target_runner_output_dir = tmp.path().join("instances");
        std::fs::create_dir(&target_runner_output_dir).unwrap();
        let metadata = BatchShimListMetadata {
            schema_version: SHIM_LIST_SCHEMA.to_string(),
            id: "list-a".to_string(),
            binary_id: "unused-by-index".to_string(),
            argv: Vec::new(),
            test_names: Vec::new(),
        };
        std::fs::write(
            target_runner_output_dir.join("list-a.list.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        let req = crate::RustCoverageBatchRequest::witness();
        let mut plan = crate::RustCoverageBatchPlan::witness();
        plan.target_runner_output_dir = target_runner_output_dir;

        let err = super::executable_index_from_list_metadata(&req, &plan).unwrap_err();

        assert!(format!("{err:?}").contains("missing list executable"));
    }

    #[cfg(unix)]
    #[test]
    fn builds_executable_index_from_target_runner_list_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("test-bin");
        std::fs::write(
            &bin,
            "#!/bin/sh\nif [ \"$1\" = \"--list\" ]; then printf 'alpha::passes: test\\nbeta::passes: test\\n'; fi\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&bin).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&bin, permissions).unwrap();

        let target_runner_output_dir = tmp.path().join("instances");
        std::fs::create_dir(&target_runner_output_dir).unwrap();
        let metadata = BatchShimListMetadata {
            schema_version: SHIM_LIST_SCHEMA.to_string(),
            id: "list-a".to_string(),
            binary_id: "unused-by-index".to_string(),
            argv: vec![bin.to_string_lossy().to_string()],
            test_names: Vec::new(),
        };
        std::fs::write(
            target_runner_output_dir.join("list-a.list.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();

        let mut req = crate::RustCoverageBatchRequest::witness();
        req.source_root = tmp.path().to_path_buf();
        req.logical_selectors = vec!["alpha::passes".to_string(), "gamma::missing".to_string()];
        req.test_args = Vec::new();
        let mut plan = crate::RustCoverageBatchPlan::witness();
        plan.target_runner_output_dir = target_runner_output_dir;

        let index = super::executable_index_from_list_metadata(&req, &plan).unwrap();
        let binary_id = crate::execute_or_reuse::batch_executor_finish::test_binary_id_for_path(&bin);

        assert_eq!(
            index.selector_binary_ids,
            std::collections::BTreeMap::from([(
                "alpha::passes".to_string(),
                vec![binary_id.clone()]
            )])
        );
        assert_eq!(index.test_binaries.len(), 1);
        assert_eq!(index.test_binaries[0].id, binary_id);
    }

    #[cfg(unix)]
    #[test]
    fn executable_index_reports_failing_list_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("test-bin");
        std::fs::write(&bin, "#!/bin/sh\necho bad >&2\nexit 7\n").unwrap();
        let mut permissions = std::fs::metadata(&bin).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&bin, permissions).unwrap();

        let kiss_profraw = tmp.path().join(".kiss").join("profraw");
        let err =
            super::list_test_names_from_executable(&bin, "bin-id", &kiss_profraw).unwrap_err();

        assert!(format!("{err:?}").contains("test binary list failed"));
        assert!(format!("{err:?}").contains("bad"));
    }

    #[cfg(unix)]
    #[test]
    fn list_test_names_sets_discard_llvm_profile_under_kiss_profraw() {
        let tmp = tempfile::tempdir().unwrap();
        let kiss_profraw = tmp.path().join(".kiss").join("profraw");
        let bin = tmp.path().join("test-bin");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"${LLVM_PROFILE_FILE:-}\" in */.kiss/profraw/default_%m_%p.profraw) ;; *) echo \"bad:$LLVM_PROFILE_FILE\" >&2; exit 9 ;; esac\nprintf 'alpha::passes: test\\n'\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&bin).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&bin, permissions).unwrap();

        let names =
            super::list_test_names_from_executable(&bin, "bin-id", &kiss_profraw).unwrap();
        assert_eq!(names, vec!["bin-id$alpha::passes".to_string()]);
        assert!(kiss_profraw.is_dir());
    }

    #[test]
    fn build_rust_test_executable_index_fails_when_run_layout_cannot_begin() {
        let tmp = tempfile::tempdir().unwrap();
        let mut req = crate::RustCoverageBatchRequest::witness();
        req.cwd = tmp.path().to_path_buf();
        req.source_root = tmp.path().to_path_buf();
        req.cache_root = tmp.path().join("cache");
        let tools = crate::plan::batch_fingerprint::RustCoverageToolIdentity {
            cargo_version: "c".into(),
            llvm_cov_version: "l".into(),
            rustc_version: "r".into(),
            cargo_nextest_version: "n".into(),
        };
        let identity = crate::plan::batch_fingerprint::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        let mut plan = crate::RustCoverageBatchPlan::witness();
        plan.build_target = tmp.path().join("build");
        plan.target_runner_output_dir = tmp.path().join("runner");
        plan.runner_map_path = tmp.path().join("runner_map.json");
        plan.generated_config = tmp.path().join("nextest.toml");
        let err =
            super::build_rust_test_executable_index(&req, &tools, &identity, &plan).unwrap_err();
        let rendered = format!("{err:?}");
        assert!(!rendered.is_empty());
    }
}
