use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_llvm_cov_runner::{
    CheckAggregateRepairPublication, CoverageOutputMode, RustCoverageBatchRequest,
    RustCoverageBatchResult, RustCoverageToolIdentity, RustTestExecutableIndex,
    build_rust_coverage_batch_plan, build_rust_test_executable_index, execute_rust_coverage_batch,
    resolve_batch_request_runners, validate_supported_rust_test_args,
};

use crate::test_runner::last_status::rust_last_status_identity;
use crate::test_runner::runners::{
    SelectorExecutionSummary, kiss_test_report_id, rust_logical_to_kiss_test_ids,
};
use crate::test_runner::rust_coverage_index::relevant_rust_batch_env;

pub(crate) mod error;
use error::map_rust_llvm_cov_error;

mod finish;
mod witness;
pub(crate) use finish::{
    cached_summary_from_check_aggregate_population, finish_rust_coverage_batch_result,
};
use witness::publish_rust_witness_after_batch;

pub(crate) fn validate_rust_extra_args(extra: &[String]) -> Result<(), String> {
    validate_supported_rust_test_args(extra)
}

pub(crate) fn run_rust_llvm_cov_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        RustCoverageRunOptions {
            extra,
            force_rerun,
            jobs,
            population_publication_selectors,
            coverage_output_mode: CoverageOutputMode::SelectorEntries,
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
    )
}

#[allow(dead_code)] // retained for CheckAggregate population callers outside `kiss test`
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
    )
}

#[allow(dead_code)] // retained warm helper; production path uses ensure kernel Accept
pub(crate) fn cached_rust_check_aggregate_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
) -> Result<Option<SelectorExecutionSummary>, String> {
    let cache_root = repo_root.join(".kiss").join("rust_llvm_cov_cache");
    // Shared execution witness is the warm authority (no llvm-cov export).
    if cache_root.join("execution_witness.json").is_file()
        || cache_root.join("index.json").is_file()
    {
        let identity =
            crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
                repo_root, extra,
            )?;
        if let Some(summary) = crate::test_runner::execution_witness::try_warm_rust_cached_summary(
            repo_root, selectors, &identity,
        ) {
            return Ok(Some(summary));
        }
    }
    // Legacy check-aggregate shortcut: only when population is explicitly marked
    // and selector universe matches (never with expected_selectors = None).
    if !cache_root.join("index.json").is_file() {
        return Ok(None);
    }
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        repo_root, extra,
    )?;
    let Some(population) = rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        &identity,
        Some(selectors),
    ) else {
        return Ok(None);
    };
    Ok(cached_summary_from_check_aggregate_population(
        repo_root,
        selectors,
        &population,
    ))
}

fn run_rust_llvm_cov_check_aggregate_selectors_with_publication(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    publication_binary_ids: Option<std::collections::BTreeSet<String>>,
    repair_publication: Option<CheckAggregateRepairPublication>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_llvm_cov_selectors_with_deps(
        repo_root,
        selectors,
        RustCoverageRunOptions {
            extra,
            force_rerun: true,
jobs,
            population_publication_selectors,
            coverage_output_mode: CoverageOutputMode::CheckAggregate {
                publication_binary_ids,
                repair_publication,
            },
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
    pub(crate) jobs: usize,
    pub(crate) population_publication_selectors: Option<Vec<String>>,
    pub(crate) coverage_output_mode: CoverageOutputMode,
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
    let batch_req = rust_coverage_batch_request_from_parts(
        repo_root,
        selectors,
        options.extra,
        options.force_rerun,
        options.jobs,
        options.population_publication_selectors,
        options.coverage_output_mode,
    )?;
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
    let result = execute_batch(&batch_req, &versions)?;
    let summary = finish_rust_coverage_batch_result(repo_root, &identity, result)?;
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

pub(crate) fn rust_coverage_batch_request_from_parts(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
    population_publication_selectors: Option<Vec<String>>,
    coverage_output_mode: CoverageOutputMode,
) -> Result<RustCoverageBatchRequest, String> {
    validate_supported_rust_test_args(extra)?;
    let gate = kiss::GateConfig::load();
    // Map logical nextest ids → PATH::symbol before applying path-pattern limits.
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    let selector_timeout_millis = selectors
        .iter()
        .map(|selector| {
            let for_limit = kiss_test_report_id(&report_ids, selector);
            let secs = kiss::limit_for_selector(&gate.max_unit_test_seconds, &for_limit);
            let millis = if secs.is_finite() && secs > 0.0 {
                (secs * 1000.0).round().clamp(1.0, u64::MAX as f64) as u64
            } else {
                0
            };
            (selector.clone(), millis)
        })
        .collect();
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
        jobs,
        generated_config: unique_rust_coverage_batch_config_path(repo_root),
        population_publication_selectors,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
        coverage_output_mode,
        selector_timeout_millis,
    };
    resolve_batch_request_runners(&mut req).map_err(map_rust_llvm_cov_error)?;
    Ok(req)
}

#[allow(dead_code)]
pub(crate) struct RustExecutableIndexBuild {
    pub(crate) request: RustCoverageBatchRequest,
    pub(crate) tools: RustCoverageToolIdentity,
    pub(crate) identity: rust_llvm_cov_runner::RustCoverageBatchIdentity,
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
    )?;
    let plan = build_rust_coverage_batch_plan(&request)?;
    let versions = detect_rust_coverage_tool_versions(repo_root)?;
    let tools = rust_coverage_tool_identity_from_versions(&versions);
    let identity = rust_llvm_cov_runner::batch_identity(&request, &tools)
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

fn unique_rust_coverage_batch_config_path(repo_root: &Path) -> PathBuf {
    static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(0);
    let run_id = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    repo_root
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join(format!(
            "run-{}-{timestamp_nanos}-{run_id}",
            std::process::id()
        ))
        .join("nextest.toml")
}

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
