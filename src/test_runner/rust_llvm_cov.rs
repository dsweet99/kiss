use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_llvm_cov_runner::{
    CheckAggregateRepairPublication, CoverageOutputMode, RustCovCacheStatus,
    RustCoverageBatchRequest, RustCoverageBatchResult, RustCoverageToolIdentity,
    RustLlvmCovOutcome, RustTestExecutableIndex, build_rust_coverage_batch_plan,
    build_rust_test_executable_index, execute_rust_coverage_batch, resolve_batch_request_runners,
    validate_supported_rust_test_args,
};

use super::last_status::{LastStatusIdentity, record_statuses, rust_last_status_identity};
use super::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};
use crate::test_runner::rust_coverage_index::relevant_rust_batch_env;
use crate::test_runner::rust_llvm_cov_error::map_rust_llvm_cov_error;

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

pub(crate) fn cached_rust_check_aggregate_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
) -> Result<Option<SelectorExecutionSummary>, String> {
    let cache_root = repo_root.join(".kiss").join("rust_llvm_cov_cache");
    // No derived population index ⇒ check-aggregate warm path cannot hit. Avoid
    // cargo -vV / toolchain probes that current_rust_coverage_batch_identity pays.
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
        selectors,
        &population,
    ))
}

fn cached_summary_from_check_aggregate_population(
    selectors: &[String],
    population: &rust_llvm_cov_runner::RustPopulationState,
) -> Option<SelectorExecutionSummary> {
    if !population
        .entries_fingerprint
        .starts_with("check-aggregate:")
    {
        return None;
    }
    let mut summary = SelectorExecutionSummary::default();
    for selector in selectors {
        println!("PASS (cached): {selector}");
        summary.record(SelectorExecutionRecord {
            selector: selector.clone(), status: rpytest_runner::TestStatus::Passed,
            cache_record: SelectorCacheRecord::Hit, exit_code: Some(0),
            duration: std::time::Duration::ZERO,
        });
    }
    Some(summary)
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
    finish_rust_coverage_batch_result(repo_root, &identity, result)
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

fn print_rust_llvm_cov_outcome(
    outcome: &RustLlvmCovOutcome,
    gate: &kiss::GateConfig,
) -> rpytest_runner::TestStatus {
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        &outcome.selector,
        outcome.duration,
        gate,
    );
    let (cache_tag, show_duration) = match outcome.cache_status {
        RustCovCacheStatus::Hit => (Some("cached"), false),
        RustCovCacheStatus::MissStored => (None, true),
        RustCovCacheStatus::FreshUnstored => (Some("not cached"), true),
    };
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        &outcome.selector,
        outcome.duration,
        cache_tag,
        show_duration,
    );
    if matches!(
        status,
        rpytest_runner::TestStatus::Failed | rpytest_runner::TestStatus::TimedOut
    ) && outcome.cache_status != RustCovCacheStatus::Hit
        && let Some(stderr) = &outcome.stderr
        && !stderr.is_empty()
    {
        eprint!("{}", String::from_utf8_lossy(stderr));
    }
    status
}

fn finish_rust_coverage_batch_result(
    repo_root: &Path,
    identity: &LastStatusIdentity,
    result: RustCoverageBatchResult,
) -> Result<SelectorExecutionSummary, String> {
    let mut summary = SelectorExecutionSummary::default();
    summary.record_rust_batch_counters(&result.counters);
    let gate = kiss::GateConfig::load();
    let mut statuses = Vec::new();
    for outcome in &result.completed {
        let status = print_rust_llvm_cov_outcome(outcome, &gate);
        statuses.push((outcome.selector.clone(), status));
        summary.record(SelectorExecutionRecord {
            selector: outcome.selector.clone(),
            status,
            cache_record: match outcome.cache_status {
                RustCovCacheStatus::Hit => SelectorCacheRecord::Hit,
                RustCovCacheStatus::MissStored => SelectorCacheRecord::MissStored,
                RustCovCacheStatus::FreshUnstored => SelectorCacheRecord::MissUnstored,
            },
            exit_code: outcome.exit_code,
            duration: outcome.duration,
        });
    }
    record_statuses(repo_root, kiss::Language::Rust, identity, &statuses)?;
    if let Some(err) = result.batch_error {
        return Err(map_rust_llvm_cov_error(err));
    }
    Ok(summary)
}

#[cfg(test)]
#[path = "rust_llvm_cov_metrics_test.rs"]
mod metrics_tests;
#[cfg(test)]
#[path = "rust_llvm_cov_test.rs"]
mod tests;
#[cfg(test)]
#[path = "rust_llvm_cov_b_test.rs"]
mod tests_b;
