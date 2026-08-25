use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::check_runtime_refresh_apply::{
    apply_identity_only_repair_labeled, apply_rerun_repair_labeled,
};
use super::{CoverageRefreshError, CoverageRefreshStats};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CheckAggregateRepairDecision {
    FullRefresh,
    IdentityOnly {
        prior_generation: String,
        retained_binary_line_maps: BTreeMap<String, kiss::rust_llvm_cov_runner::RustLineCoverage>,
    },
    Rerun {
        prior_generation: String,
        rerun_selectors: Vec<String>,
        replacement_binary_ids: BTreeSet<String>,
        retained_binary_line_maps: BTreeMap<String, kiss::rust_llvm_cov_runner::RustLineCoverage>,
    },
}

pub(super) fn try_repair_rust_check_aggregate_labeled(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
    caller_label: &str,
) -> Result<Option<CoverageRefreshStats>, CoverageRefreshError> {
    let current_identity =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &[],
        )
        .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    let Some(prior) = kiss::rust_llvm_cov_runner::load_reusable_prior_check_aggregate(
        &cache_root,
        repo_root,
        selectors,
        &current_identity.selection_context_fingerprint,
    ) else {
        return Ok(None);
    };
    match kiss::rust_llvm_cov_runner::reusable_check_aggregate_delta(
        repo_root,
        &prior.ordinary_source_digests,
        &current_identity.ordinary_source_digests,
    ) {
        kiss::rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange => return Ok(None),
        kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged
        | kiss::rust_llvm_cov_runner::RustSnapshotDelta::Modified(_) => {}
    }
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        repo_root,
        selectors,
        &[],
        jobs,
    )
    .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let decision = classify_check_aggregate_repair(
        selectors,
        &prior,
        &build.index.selector_binary_ids,
        &build.index.test_binaries,
    );
    let decision = maybe_downgrade_rerun_when_witness_warm(
        repo_root,
        selectors,
        &current_identity,
        &prior,
        &build.index.selector_binary_ids,
        decision,
    );
    match decision {
        CheckAggregateRepairDecision::FullRefresh => Ok(None),
        CheckAggregateRepairDecision::IdentityOnly {
            prior_generation,
            retained_binary_line_maps,
        } => Ok(Some(apply_identity_only_repair_labeled(
            repo_root,
            ignore,
            &build,
            selectors,
            &prior_generation,
            retained_binary_line_maps,
            caller_label,
        )?)),
        CheckAggregateRepairDecision::Rerun {
            prior_generation,
            rerun_selectors,
            replacement_binary_ids,
            retained_binary_line_maps,
        } => Ok(Some(apply_rerun_repair_labeled(
            super::check_runtime_refresh_apply::RerunRepairArgs {
                repo_root,
                ignore,
                build: &build,
                prior_generation: &prior_generation,
                rerun_selectors,
                replacement_binary_ids,
                retained_binary_line_maps,
                jobs,
                caller_label,
            },
        )?)),
    }
}

pub(crate) fn maybe_downgrade_rerun_when_witness_warm(
    repo_root: &Path,
    selectors: &[String],
    current_identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    decision: CheckAggregateRepairDecision,
) -> CheckAggregateRepairDecision {
    let CheckAggregateRepairDecision::Rerun {
        prior_generation, ..
    } = &decision
    else {
        return decision;
    };
    if crate::test_runner::execution_witness::try_warm_rust_cached_summary(
        repo_root,
        selectors,
        current_identity,
        &kiss::GateConfig::load_for_repo(repo_root),
    )
    .is_none()
    {
        return decision;
    }
    let current_mapped = current_mapped_binary_ids(current_selector_binary_ids);
    CheckAggregateRepairDecision::IdentityOnly {
        prior_generation: prior_generation.clone(),
        retained_binary_line_maps: retained_maps_ignoring_digest_mismatch(prior, &current_mapped),
    }
}

pub(crate) fn retained_maps_ignoring_digest_mismatch(
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_mapped_binary_ids: &BTreeSet<String>,
) -> BTreeMap<String, kiss::rust_llvm_cov_runner::RustLineCoverage> {
    prior
        .binaries
        .iter()
        .filter(|(binary_id, _)| current_mapped_binary_ids.contains(*binary_id))
        .map(|(binary_id, record)| {
            (
                binary_id.clone(),
                kiss::rust_llvm_cov_runner::RustLineCoverage {
                    files: record.line_map.clone(),
                },
            )
        })
        .collect()
}

pub(crate) fn classify_check_aggregate_repair(
    selectors: &[String],
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    current_test_binaries: &[kiss::rust_llvm_cov_runner::RustTestBinaryIdentity],
) -> CheckAggregateRepairDecision {
    let current_binary_digests = current_binary_digests(current_test_binaries);
    let current_mapped_binary_ids = current_mapped_binary_ids(current_selector_binary_ids);
    let mut replacement_binary_ids =
        changed_or_new_binary_ids(prior, &current_binary_digests, &current_mapped_binary_ids);
    if !classify_changed_selector_mappings(
        selectors,
        prior,
        current_selector_binary_ids,
        &mut replacement_binary_ids,
    ) {
        return CheckAggregateRepairDecision::FullRefresh;
    }
    let retained_binary_line_maps = retained_binary_line_maps(
        prior,
        &current_binary_digests,
        &current_mapped_binary_ids,
        &replacement_binary_ids,
    );
    if replacement_binary_ids.is_empty() {
        return CheckAggregateRepairDecision::IdentityOnly {
            prior_generation: prior.generation_fingerprint.clone(),
            retained_binary_line_maps,
        };
    }
    let rerun_selectors = rerun_selectors_for_replacements(
        selectors,
        current_selector_binary_ids,
        &replacement_binary_ids,
    );
    if rerun_selectors.is_empty() {
        return CheckAggregateRepairDecision::FullRefresh;
    }
    CheckAggregateRepairDecision::Rerun {
        prior_generation: prior.generation_fingerprint.clone(),
        rerun_selectors,
        replacement_binary_ids,
        retained_binary_line_maps,
    }
}

fn current_binary_digests(
    current_test_binaries: &[kiss::rust_llvm_cov_runner::RustTestBinaryIdentity],
) -> BTreeMap<String, String> {
    current_test_binaries
        .iter()
        .map(|binary| (binary.id.clone(), binary.digest.clone()))
        .collect()
}

fn current_mapped_binary_ids(
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<String> {
    current_selector_binary_ids
        .values()
        .flatten()
        .cloned()
        .collect()
}

fn changed_or_new_binary_ids(
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_binary_digests: &BTreeMap<String, String>,
    current_mapped_binary_ids: &BTreeSet<String>,
) -> BTreeSet<String> {
    current_binary_digests
        .iter()
        .filter(|(binary_id, _)| current_mapped_binary_ids.contains(*binary_id))
        .filter(|(binary_id, digest)| prior_binary_changed(prior, binary_id, digest))
        .map(|(binary_id, _)| binary_id.clone())
        .collect()
}

fn prior_binary_changed(
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    binary_id: &str,
    digest: &str,
) -> bool {
    prior
        .binaries
        .get(binary_id)
        .is_none_or(|prior_binary| prior_binary.digest != digest)
}

fn classify_changed_selector_mappings(
    selectors: &[String],
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    replacement_binary_ids: &mut BTreeSet<String>,
) -> bool {
    for selector in selectors {
        let Some(current_ids) = current_selector_binary_ids.get(selector) else {
            return false;
        };
        if current_ids.is_empty() {
            return false;
        }
        let Some(prior_ids) = prior.selector_binary_ids.get(selector) else {
            return false;
        };
        if prior_ids != current_ids {
            replacement_binary_ids.extend(current_ids.iter().cloned());
        }
    }
    true
}

fn retained_binary_line_maps(
    prior: &kiss::rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_binary_digests: &BTreeMap<String, String>,
    current_mapped_binary_ids: &BTreeSet<String>,
    replacement_binary_ids: &BTreeSet<String>,
) -> BTreeMap<String, kiss::rust_llvm_cov_runner::RustLineCoverage> {
    prior
        .binaries
        .iter()
        .filter(|(binary_id, record)| {
            current_mapped_binary_ids.contains(*binary_id)
                && !replacement_binary_ids.contains(*binary_id)
                && current_binary_digests.get(*binary_id) == Some(&record.digest)
        })
        .map(|(binary_id, record)| {
            (
                binary_id.clone(),
                kiss::rust_llvm_cov_runner::RustLineCoverage {
                    files: record.line_map.clone(),
                },
            )
        })
        .collect()
}

fn rerun_selectors_for_replacements(
    selectors: &[String],
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    replacement_binary_ids: &BTreeSet<String>,
) -> Vec<String> {
    selectors
        .iter()
        .filter(|selector| {
            selector_intersects_replacements(
                selector,
                current_selector_binary_ids,
                replacement_binary_ids,
            )
        })
        .cloned()
        .collect()
}

fn selector_intersects_replacements(
    selector: &str,
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    replacement_binary_ids: &BTreeSet<String>,
) -> bool {
    current_selector_binary_ids
        .get(selector)
        .is_some_and(|ids| ids.iter().any(|id| replacement_binary_ids.contains(id)))
}
