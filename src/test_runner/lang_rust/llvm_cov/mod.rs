use kiss::rust_llvm_cov_runner::{
    CheckAggregateRepairPublication, CoverageOutputMode, RustCoverageBatchRequest,
    RustCoverageBatchResult, RustCoverageToolIdentity, RustTestExecutableIndex,
    build_rust_coverage_batch_plan, build_rust_test_executable_index, execute_rust_coverage_batch,
    resolve_batch_request_runners, validate_supported_rust_test_args,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::test_runner::last_status::rust_last_status_identity;
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::rust_coverage_index::relevant_rust_batch_env;

pub(crate) mod error;
use error::map_rust_llvm_cov_error;

mod timeout;
use timeout::selector_timeout_millis_for_batch;

mod finish;
mod live_status;
mod witness;
pub(crate) use finish::{
    cached_summary_from_check_aggregate_population, finish_rust_coverage_batch_result,
};
use live_status::install_live_rust_status_hook;
use witness::publish_rust_witness_after_batch;

pub(crate) fn validate_rust_extra_args(extra: &[String]) -> Result<(), String> {
    validate_supported_rust_test_args(extra)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_rust_llvm_cov_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    force_rerun_selectors: &[String],
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    gate: &kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        RustCoverageRunOptions {
            extra,
            force_rerun,
            force_rerun_selectors,
            jobs,
            population_publication_selectors,
            coverage_output_mode: CoverageOutputMode::SelectorEntries,
            gate: gate.clone(),
        },
        detect_rust_coverage_tool_versions,
        execute_rust_coverage_batch_compat,
    )
}

pub(crate) fn run_rust_llvm_cov_check_aggregate_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
    publication_binary_ids: Option<std::collections::BTreeSet<String>>,
    repair_publication: Option<CheckAggregateRepairPublication>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_check_aggregate_selectors_with_publication(
        repo_root,
        selectors,
        extra,
        jobs,
        None,
        publication_binary_ids,
        repair_publication,
        kiss::GateConfig::load_for_repo(repo_root),
    )
}

pub(crate) fn run_rust_llvm_cov_check_aggregate_selectors_with_gate(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
    publication_binary_ids: Option<std::collections::BTreeSet<String>>,
    repair_publication: Option<CheckAggregateRepairPublication>,
    gate: &kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_check_aggregate_selectors_with_publication(
        repo_root,
        selectors,
        extra,
        jobs,
        None,
        publication_binary_ids,
        repair_publication,
        gate.clone(),
    )
}

#[allow(dead_code)]
pub(crate) fn run_rust_llvm_cov_check_aggregate_population_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
    population_publication_selectors: Vec<String>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_check_aggregate_selectors_with_publication(
        repo_root,
        selectors,
        extra,
        jobs,
        Some(population_publication_selectors),
        None,
        None,
        kiss::GateConfig::load_for_repo(repo_root),
    )
}

#[allow(dead_code)]
pub(crate) fn cached_rust_check_aggregate_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
) -> Result<Option<SelectorExecutionSummary>, String> {
    let cache_root = repo_root.join(".kiss").join("rust_llvm_cov_cache");

    if cache_root.join("execution_witness.json").is_file()
        || cache_root.join("index.json").is_file()
        || cache_root.join("current_generation.json").is_file()
    {
        let identity =
            crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
                repo_root, extra,
            )?;
        let binaries_are_current =
            kiss::rust_llvm_cov_runner::current_population_manifest_test_binaries_match(
                &cache_root,
                repo_root,
                &identity,
            )
            .unwrap_or(false);
        if binaries_are_current
            && let Some(summary) =
                crate::test_runner::execution_witness::try_warm_rust_cached_summary(
                    repo_root,
                    selectors,
                    &identity,
                    &kiss::GateConfig::load_for_repo(repo_root),
                )
        {
            return Ok(Some(summary));
        }
    }

    if !cache_root.join("index.json").is_file() {
        return Ok(None);
    }
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        repo_root, extra,
    )?;
    let Some(population) = kiss::rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        Some(selectors),
    ) else {
        return Ok(None);
    };
    cached_summary_from_check_aggregate_population(repo_root, selectors, &population)
}

#[allow(clippy::too_many_arguments)]
fn run_rust_llvm_cov_check_aggregate_selectors_with_publication(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    publication_binary_ids: Option<std::collections::BTreeSet<String>>,
    repair_publication: Option<CheckAggregateRepairPublication>,
    gate: kiss::GateConfig,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        RustCoverageRunOptions {
            extra,
            force_rerun: false,
            force_rerun_selectors: &[],
            jobs,
            population_publication_selectors: population_publication_selectors
                .or_else(|| Some(selectors.to_vec())),
            coverage_output_mode: CoverageOutputMode::CheckAggregate {
                publication_binary_ids,
                repair_publication,
            },
            gate,
        },
        detect_rust_coverage_tool_versions,
        execute_rust_coverage_batch_compat,
    )
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RustCoverageToolVersions {
    pub(crate) cargo: String,
    pub(crate) llvm_cov: String,
    pub(crate) rustc: String,
    pub(crate) cargo_nextest: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RustCoverageRunOptions<'a> {
    pub(crate) extra: &'a [String],
    pub(crate) force_rerun: bool,
    pub(crate) force_rerun_selectors: &'a [String],
    pub(crate) jobs: usize,
    pub(crate) population_publication_selectors: Option<Vec<String>>,
    pub(crate) coverage_output_mode: CoverageOutputMode,
    pub(crate) gate: kiss::GateConfig,
}

pub(crate) fn run_rust_llvm_cov_selectors_with_deps<D, E>(
    repo_root: &Path,
    selectors: &[String],
    options: RustCoverageRunOptions<'_>,
    detect_versions: D,
    execute_batch: E,
) -> Result<SelectorExecutionSummary, String>
where
    D: FnOnce(&Path) -> Result<RustCoverageToolVersions, String>,
    E: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolVersions,
    ) -> Result<RustCoverageBatchResult, String>,
{
    assert!(options.jobs > 0, "jobs must be greater than zero");
    validate_supported_rust_test_args(options.extra)?;
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let stage_started = std::time::Instant::now();
    crate::test_runner::emit_test_progress("kiss test: Running batch-request");
    let request_started = std::time::Instant::now();
    let mut batch_req = rust_coverage_batch_request_from_parts(
        repo_root,
        selectors,
        options.extra,
        options.force_rerun,
        options.jobs,
        options.population_publication_selectors,
        options.coverage_output_mode,
        &options.gate,
    )?;
    batch_req.force_rerun_selectors = options.force_rerun_selectors.to_vec();
    crate::test_runner::emit_test_progress(&format!(
        "kiss test: Ran batch-request {:.1}ms",
        request_started.elapsed().as_secs_f64() * 1000.0
    ));
    build_rust_coverage_batch_plan(&batch_req)?;
    let versions = detect_versions(repo_root)?;
    let identity = rust_last_status_identity(
        &versions.cargo,
        &versions.llvm_cov,
        &versions.rustc,
        &versions.cargo_nextest,
        options.extra,
        &batch_req.runner_map_fingerprint,
    );
    install_live_rust_status_hook(repo_root, selectors, &options.gate, &identity)?;
    let result = execute_batch(&batch_req, &versions);
    let live_err = kiss::rust_llvm_cov_runner::take_live_rust_error();
    kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
    if let Some(err) = live_err {
        eprintln!("{err}");
    }
    let result = result?;
    crate::test_runner::emit_stage_time("rust_llvm_cov", stage_started.elapsed());
    let summary = finish_rust_coverage_batch_result(repo_root, &identity, result, &options.gate)?;
    live_status::finish_live_rust_remaining();
    publish_rust_witness_after_batch(repo_root, &batch_req, &summary)?;
    Ok(summary)
}

fn execute_rust_coverage_batch_compat(
    batch_req: &RustCoverageBatchRequest,
    versions: &RustCoverageToolVersions,
) -> Result<RustCoverageBatchResult, String> {
    let tools = RustCoverageToolIdentity {
        cargo_version: versions.cargo.clone(),
        llvm_cov_version: versions.llvm_cov.clone(),
        rustc_version: versions.rustc.clone(),
        cargo_nextest_version: versions.cargo_nextest.clone(),
    };
    execute_rust_coverage_batch(batch_req, &tools).map_err(map_rust_llvm_cov_error)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rust_coverage_batch_request_from_parts(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    coverage_output_mode: CoverageOutputMode,
    gate: &kiss::GateConfig,
) -> Result<RustCoverageBatchRequest, String> {
    validate_supported_rust_test_args(extra)?;
    let selector_timeout_millis =
        selector_timeout_millis_for_batch(repo_root, selectors, &coverage_output_mode, gate)?;
    let mut req = RustCoverageBatchRequest {
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo_root.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: extra.to_vec(),
        env: relevant_rust_batch_env(),
        force_rerun,
        force_rerun_selectors: Vec::new(),
        jobs,
        generated_config: unique_rust_coverage_batch_config_path(repo_root),
        population_publication_selectors,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
        coverage_output_mode,
        selector_timeout_millis,
        cache_policy: kiss::TestSectionConfig::try_load_path_only(&kiss::kissconfig_path_for_repo(
            repo_root,
        ))
        .map(|config| config.cache_policy)
        .unwrap_or_default(),
    };
    resolve_batch_request_runners(&mut req).map_err(map_rust_llvm_cov_error)?;
    Ok(req)
}

#[allow(dead_code)]
pub(crate) struct RustExecutableIndexBuild {
    pub(crate) request: RustCoverageBatchRequest,
    pub(crate) tools: RustCoverageToolIdentity,
    pub(crate) identity: kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    pub(crate) index: RustTestExecutableIndex,
}

#[allow(dead_code)]
pub(crate) fn build_current_rust_test_executable_index(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
) -> Result<RustExecutableIndexBuild, String> {
    validate_supported_rust_test_args(extra)?;
    let request = rust_coverage_batch_request_from_parts(
        repo_root,
        selectors,
        extra,
        false,
        jobs,
        Some(selectors.to_vec()),
        CoverageOutputMode::SelectorEntries,
        &kiss::GateConfig::load_for_repo(repo_root),
    )?;
    let plan = build_rust_coverage_batch_plan(&request)?;
    let versions = detect_rust_coverage_tool_versions(repo_root)?;
    let tools = rust_coverage_tool_identity_from_versions(&versions);
    let identity = kiss::rust_llvm_cov_runner::batch_identity(&request, &tools)
        .map_err(|err| format!("batch identity: {err}"))?;
    let index = build_rust_test_executable_index(&request, &tools, &identity, &plan)
        .map_err(map_rust_llvm_cov_error)?;
    Ok(RustExecutableIndexBuild {
        request,
        tools,
        identity,
        index,
    })
}

#[allow(dead_code)]
pub(crate) fn rust_coverage_tool_identity_from_versions(
    versions: &RustCoverageToolVersions,
) -> RustCoverageToolIdentity {
    RustCoverageToolIdentity {
        cargo_version: versions.cargo.clone(),
        llvm_cov_version: versions.llvm_cov.clone(),
        rustc_version: versions.rustc.clone(),
        cargo_nextest_version: versions.cargo_nextest.clone(),
    }
}

mod run_id;
use run_id::unique_rust_coverage_batch_config_path;

pub(crate) fn detect_rust_coverage_tool_versions(
    repo_root: &Path,
) -> Result<RustCoverageToolVersions, String> {
    let (cargo, llvm_cov, rustc, cargo_nextest) =
        crate::test_runner::rust_coverage_index::rust_coverage_tool_versions_from_cache_or_detect(
            repo_root,
        )?;
    Ok(RustCoverageToolVersions {
        cargo,
        llvm_cov,
        rustc,
        cargo_nextest,
    })
}

#[cfg(test)]
#[path = "metrics_test.rs"]
mod metrics_tests;
#[cfg(test)]
#[path = "llvm_cov_test.rs"]
mod tests;
#[cfg(test)]
#[path = "llvm_cov_b_test.rs"]
mod tests_b;
#[cfg(test)]
#[path = "llvm_cov_c_test.rs"]
mod tests_c;
