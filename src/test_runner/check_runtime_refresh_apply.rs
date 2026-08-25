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
    prior_generation: &str,
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        kiss::rust_llvm_cov_runner::RustLineCoverage,
    >,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    apply_identity_only_repair_labeled(
        repo_root,
        ignore,
        build,
        selectors,
        prior_generation,
        retained_binary_line_maps,
        "kiss test",
    )
}

pub(crate) fn apply_identity_only_repair_labeled(
    repo_root: &Path,
    ignore: &[String],
    build: &crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    selectors: &[String],
    prior_generation: &str,
    retained_binary_line_maps: std::collections::BTreeMap<
        String,
        kiss::rust_llvm_cov_runner::RustLineCoverage,
    >,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    kiss::rust_llvm_cov_runner::rekey_selector_entries_to_identity(
        &build.request,
        &build.tools,
        &build.identity,
        prior_generation,
        selectors,
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    let aggregate = kiss::rust_llvm_cov_runner::build_check_aggregate(
        &build.request,
        &build.identity,
        selectors,
        build.index.selector_binary_ids.clone(),
        &build.index.test_binaries,
        retained_binary_line_maps,
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    kiss::rust_llvm_cov_runner::publish_check_aggregate(&build.request, &aggregate)
        .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    load_rust_runtime_coverage(repo_root, ignore, &kiss::GateConfig::default())
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
    eprintln!(
        "{caller_label}: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports=0",
        aggregate.binaries.len()
    );
    Ok(CoverageRefreshStats::for_rust(
        super::LanguageRefreshStats {
            aggregate_binaries: aggregate.binaries.len(),
            identity_only_repair: true,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
pub(crate) fn finalize_population_summary(
    repo_root: &Path,
    ignore: &[String],
    summary: &SelectorExecutionSummary,
    full_refresh: bool,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    finalize_population_summary_labeled(repo_root, ignore, summary, full_refresh, "kiss test")
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
    load_rust_runtime_coverage(repo_root, ignore, &kiss::GateConfig::default())
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
    Ok(CoverageRefreshStats::for_rust(
        super::LanguageRefreshStats {
            test_instances: summary.rust_test_instances,
            aggregate_binaries: summary.rust_aggregate_binaries,
            aggregate_exports: summary.rust_aggregate_exports,
            full_refresh,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
pub(crate) fn apply_rerun_repair(
    args: RerunRepairArgs<'_>,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    apply_rerun_repair_labeled(args)
}

pub(crate) struct RerunRepairArgs<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) ignore: &'a [String],
    pub(crate) build: &'a crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    pub(crate) prior_generation: &'a str,
    pub(crate) rerun_selectors: Vec<String>,
    pub(crate) replacement_binary_ids: std::collections::BTreeSet<String>,
    pub(crate) retained_binary_line_maps:
        std::collections::BTreeMap<String, kiss::rust_llvm_cov_runner::RustLineCoverage>,
    pub(crate) jobs: usize,
    pub(crate) caller_label: &'a str,
}

pub(crate) fn apply_rerun_repair_labeled(
    args: RerunRepairArgs<'_>,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "{}: incrementally refreshing Rust runtime coverage ({} tests, {} replacement binaries)",
        args.caller_label,
        args.rerun_selectors.len(),
        args.replacement_binary_ids.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let repair_publication = kiss::rust_llvm_cov_runner::CheckAggregateRepairPublication {
        prior_generation: args.prior_generation.to_string(),
        selector_binary_ids: args.build.index.selector_binary_ids.clone(),
        test_binaries: args.build.index.test_binaries.clone(),
        retained_binary_line_maps: args.retained_binary_line_maps,
    };
    let summary = crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors(
        args.repo_root,
        &args.rerun_selectors,
        &[],
        args.jobs,
        Some(args.replacement_binary_ids),
        Some(repair_publication),
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
    finalize_population_summary_labeled(
        args.repo_root,
        args.ignore,
        &summary,
        false,
        args.caller_label,
    )
}
