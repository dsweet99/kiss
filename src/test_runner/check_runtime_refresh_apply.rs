use std::path::Path;

use crate::test_runner::check_line_coverage::load_rust_runtime_coverage;
use crate::test_runner::runners::SelectorExecutionSummary;

use super::{CoverageRefreshError, CoverageRefreshStats, ScopedRefreshEnvGuard};

#[cfg(test)]
pub(crate) fn apply_identity_only_repair(
    repo_root: &Path,
    ignore: &[String],
    build: &crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    selectors: &[String],
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        rust_llvm_cov_runner::RustLineCoverage,
    >,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    apply_identity_only_repair_labeled(repo_root, ignore, build, selectors, retained_binary_line_maps, "kiss cov")
}

pub(crate) fn apply_identity_only_repair_labeled(
    repo_root: &Path,
    ignore: &[String],
    build: &crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    selectors: &[String],
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        rust_llvm_cov_runner::RustLineCoverage,
    >,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    let aggregate = rust_llvm_cov_runner::build_check_aggregate(
        &build.request,
        &build.identity,
        selectors,
        build.index.selector_binary_ids.clone(),
        &build.index.test_binaries,
        retained_binary_line_maps,
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    rust_llvm_cov_runner::publish_check_aggregate(&build.request, &aggregate)
        .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    load_rust_runtime_coverage(repo_root, ignore)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
    eprintln!(
        "{caller_label}: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports=0",
        aggregate.binaries.len()
    );
    Ok(CoverageRefreshStats {
        rust_aggregate_binaries: aggregate.binaries.len(),
        rust_identity_only_repair: true,
        ..Default::default()
    })
}

#[cfg(test)]
pub(crate) fn finalize_population_summary(
    repo_root: &Path,
    ignore: &[String],
    summary: &SelectorExecutionSummary,
    full_refresh: bool,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    finalize_population_summary_labeled(repo_root, ignore, summary, full_refresh, "kiss cov")
}

pub(crate) fn finalize_population_summary_labeled(
    repo_root: &Path,
    ignore: &[String],
    summary: &SelectorExecutionSummary,
    full_refresh: bool,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "{caller_label}: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports={}",
        summary.rust_aggregate_binaries, summary.rust_aggregate_exports
    );
    if summary.exit_code != 0 {
        return Err(CoverageRefreshError::TestExecution {
            language: "Rust",
            total: summary.total,
            failed: summary.failed,
            exit_code: summary.exit_code,
        });
    }
    load_rust_runtime_coverage(repo_root, ignore)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
    Ok(CoverageRefreshStats {
        rust_test_instances: summary.rust_test_instances,
        rust_aggregate_binaries: summary.rust_aggregate_binaries,
        rust_aggregate_exports: summary.rust_aggregate_exports,
        rust_full_refresh: full_refresh,
        ..Default::default()
    })
}

#[cfg(test)]
pub(crate) fn apply_rerun_repair(
    repo_root: &Path,
    ignore: &[String],
    build: &crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    rerun_selectors: Vec<String>,
    replacement_binary_ids: std::collections::BTreeSet<String>,
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        rust_llvm_cov_runner::RustLineCoverage,
    >,
    jobs: usize,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    apply_rerun_repair_labeled(repo_root, ignore, build, rerun_selectors, replacement_binary_ids, retained_binary_line_maps, jobs, "kiss cov")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_rerun_repair_labeled(
    repo_root: &Path,
    ignore: &[String],
    build: &crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    rerun_selectors: Vec<String>,
    replacement_binary_ids: std::collections::BTreeSet<String>,
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        rust_llvm_cov_runner::RustLineCoverage,
    >,
    jobs: usize,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "{caller_label}: incrementally refreshing Rust runtime coverage ({} tests, {} replacement binaries)",
        rerun_selectors.len(),
        replacement_binary_ids.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let repair_publication = rust_llvm_cov_runner::CheckAggregateRepairPublication {
        selector_binary_ids: build.index.selector_binary_ids.clone(),
        test_binaries: build.index.test_binaries.clone(),
        retained_binary_line_maps,
    };
    let summary = crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors(
        repo_root,
        &rerun_selectors,
        &[],
        jobs,
        Some(replacement_binary_ids),
        Some(repair_publication),
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
    finalize_population_summary_labeled(repo_root, ignore, &summary, false, caller_label)
}
