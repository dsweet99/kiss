use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CheckAggregateRepairDecision {
    FullRefresh,
    IdentityOnly {
        retained_binary_line_maps: BTreeMap<String, rust_llvm_cov_runner::RustLineCoverage>,
    },
    Rerun {
        rerun_selectors: Vec<String>,
        replacement_binary_ids: BTreeSet<String>,
        retained_binary_line_maps: BTreeMap<String, rust_llvm_cov_runner::RustLineCoverage>,
    },
}

pub(super) fn classify_check_aggregate_repair(
    selectors: &[String],
    prior: &rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_selector_binary_ids: &BTreeMap<String, Vec<String>>,
    current_test_binaries: &[rust_llvm_cov_runner::RustTestBinaryIdentity],
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
        rerun_selectors,
        replacement_binary_ids,
        retained_binary_line_maps,
    }
}

fn current_binary_digests(
    current_test_binaries: &[rust_llvm_cov_runner::RustTestBinaryIdentity],
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
    prior: &rust_llvm_cov_runner::ValidatedCheckAggregate,
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
    prior: &rust_llvm_cov_runner::ValidatedCheckAggregate,
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
    prior: &rust_llvm_cov_runner::ValidatedCheckAggregate,
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
    prior: &rust_llvm_cov_runner::ValidatedCheckAggregate,
    current_binary_digests: &BTreeMap<String, String>,
    current_mapped_binary_ids: &BTreeSet<String>,
    replacement_binary_ids: &BTreeSet<String>,
) -> BTreeMap<String, rust_llvm_cov_runner::RustLineCoverage> {
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
                rust_llvm_cov_runner::RustLineCoverage {
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
