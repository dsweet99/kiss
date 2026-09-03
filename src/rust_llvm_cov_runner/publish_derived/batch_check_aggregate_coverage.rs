use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{
    CHECK_AGGREGATE_SCHEMA_VERSION, CheckAggregateBinaryRecord, OnDiskCheckAggregate,
    ValidatedCheckAggregate, check_aggregate_path,
};
use crate::rust_llvm_cov_runner::RustLineCoverage;

pub fn selector_coverage_from_check_aggregate_generation(
    cache_root: &Path,
    source_root: &Path,
    generation_fingerprint: &str,
) -> Option<BTreeMap<String, RustLineCoverage>> {
    let bytes = fs::read(check_aggregate_path(cache_root)).ok()?;
    let raw: OnDiskCheckAggregate = serde_json::from_slice(&bytes).ok()?;
    if raw.generation_fingerprint != generation_fingerprint {
        return None;
    }
    let mode = super::batch_check_aggregate_load::CheckAggregateLoadMode::Current {
        input_fingerprint: raw.input_fingerprint.clone(),
        generation_fingerprint: raw.generation_fingerprint.clone(),
        selection_context_fingerprint: raw.selection_context_fingerprint.clone(),
        ordinary_source_digests: raw.ordinary_source_digests.clone(),
    };
    let aggregate = super::validate_check_aggregate(raw, source_root, None, mode, true)?;
    Some(
        aggregate
            .selectors
            .iter()
            .map(|selector| {
                (
                    selector.clone(),
                    selector_coverage_from_validated(&aggregate, selector),
                )
            })
            .collect(),
    )
}

pub fn file_selector_index_from_check_aggregate_generation(
    cache_root: &Path,
    source_root: &Path,
    generation_fingerprint: &str,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let coverage = selector_coverage_from_check_aggregate_generation(
        cache_root,
        source_root,
        generation_fingerprint,
    )?;
    Some(file_selector_index_from_coverage(coverage))
}

pub fn file_selector_index_from_validated(
    aggregate: &ValidatedCheckAggregate,
) -> BTreeMap<String, BTreeSet<String>> {
    let coverage = aggregate
        .selectors
        .iter()
        .map(|selector| {
            (
                selector.clone(),
                selector_coverage_from_validated(aggregate, selector),
            )
        })
        .collect();
    file_selector_index_from_coverage(coverage)
}

fn file_selector_index_from_coverage(
    coverage: BTreeMap<String, RustLineCoverage>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (selector, coverage) in coverage {
        for (path, lines) in coverage.files {
            if !lines.is_empty() {
                index.entry(path).or_default().insert(selector.clone());
            }
        }
    }
    index
}

pub fn selector_coverage_from_validated(
    aggregate: &ValidatedCheckAggregate,
    selector: &str,
) -> RustLineCoverage {
    let mut files: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    let Some(binary_ids) = aggregate.selector_binary_ids.get(selector) else {
        return RustLineCoverage { files };
    };
    for binary_id in binary_ids {
        let Some(record) = aggregate.binaries.get(binary_id) else {
            continue;
        };
        for (path, lines) in &record.line_map {
            files
                .entry(path.clone())
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    RustLineCoverage { files }
}
