use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::publish_derived::batch_check_aggregate_identity::{
    integrity_fingerprint, stable_check_aggregate_identity, union_binary_maps, validate_line_map,
    validate_ordinary_source_digests,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::normalized_source_root;
use crate::rust_llvm_cov_runner::rust_cov_cache::{repo_relative_coverage_file, repo_relative_path};
use crate::rust_llvm_cov_runner::{
    CACHE_SCHEMA_VERSION, RustLineCoverage, RustLlvmCovError, RustSnapshotDelta,
    RustTestBinaryIdentity,
};

#[path = "batch_check_aggregate_coverage.rs"]
mod batch_check_aggregate_coverage;
pub use batch_check_aggregate_coverage::{
    selector_coverage_from_check_aggregate_generation, selector_coverage_from_validated,
};

pub const CHECK_AGGREGATE_SCHEMA_VERSION: &str = "rust-check-aggregate-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckAggregateSnapshot {
    pub identity: String,
    pub covered_lines: BTreeMap<String, BTreeSet<u32>>,
    pub aggregate: ValidatedCheckAggregate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedCheckAggregate {
    pub input_fingerprint: String,
    pub generation_fingerprint: String,
    pub selection_context_fingerprint: String,
    pub ordinary_source_digests: BTreeMap<String, String>,
    pub selectors: Vec<String>,
    pub selector_binary_ids: BTreeMap<String, Vec<String>>,
    pub binaries: BTreeMap<String, CheckAggregateBinaryRecord>,
    pub aggregate_covered_lines: BTreeMap<String, BTreeSet<u32>>,
    pub integrity_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckAggregateBinaryRecord {
    pub id: String,
    pub executable: String,
    pub digest: String,
    pub line_map: BTreeMap<String, BTreeSet<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OnDiskCheckAggregate {
    schema_version: String,
    cache_schema_version: String,
    source_root: String,
    input_fingerprint: String,
    generation_fingerprint: String,
    selection_context_fingerprint: String,
    ordinary_source_digests: BTreeMap<String, String>,
    selectors: Vec<String>,
    selector_binary_ids: BTreeMap<String, Vec<String>>,
    binaries: Vec<CheckAggregateBinaryRecord>,
    aggregate_covered_lines: BTreeMap<String, BTreeSet<u32>>,
    integrity_fingerprint: String,
}

pub fn load_current_check_aggregate_snapshot(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<CheckAggregateSnapshot> {
    let aggregate = load_validated_check_aggregate(
        cache_root,
        source_root,
        selectors,
        CheckAggregateLoadMode::Current {
            input_fingerprint: identity.input_digest.clone(),
            generation_fingerprint: identity.generation_fingerprint.clone(),
            selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        },
    )?;
    let snapshot_identity = stable_check_aggregate_identity(&aggregate);
    Some(CheckAggregateSnapshot {
        identity: snapshot_identity,
        covered_lines: aggregate.aggregate_covered_lines.clone(),
        aggregate,
    })
}

pub fn load_reusable_prior_check_aggregate(
    cache_root: &Path,
    source_root: &Path,
    selectors: &[String],
    selection_context_fingerprint: &str,
) -> Option<ValidatedCheckAggregate> {
    load_validated_check_aggregate(
        cache_root,
        source_root,
        Some(selectors),
        CheckAggregateLoadMode::ReusablePrior {
            selection_context_fingerprint: selection_context_fingerprint.to_string(),
        },
    )
}

enum CheckAggregateLoadMode {
    Current {
        input_fingerprint: String,
        generation_fingerprint: String,
        selection_context_fingerprint: String,
    },
    ReusablePrior {
        selection_context_fingerprint: String,
    },
}

fn load_validated_check_aggregate(
    cache_root: &Path,
    source_root: &Path,
    selectors: Option<&[String]>,
    mode: CheckAggregateLoadMode,
) -> Option<ValidatedCheckAggregate> {
    let bytes = fs::read(check_aggregate_path(cache_root)).ok()?;
    let raw: OnDiskCheckAggregate = serde_json::from_slice(&bytes).ok()?;
    validate_check_aggregate(raw, source_root, selectors, mode)
}

fn validate_check_aggregate(
    mut raw: OnDiskCheckAggregate,
    source_root: &Path,
    selectors: Option<&[String]>,
    mode: CheckAggregateLoadMode,
) -> Option<ValidatedCheckAggregate> {
    raw_identity_is_valid(&raw, source_root, &mode).then_some(())?;
    selector_population_is_valid(&raw, selectors).then_some(())?;
    validate_ordinary_source_digests(&raw.ordinary_source_digests).then_some(())?;
    let binaries = validated_binary_records(source_root, std::mem::take(&mut raw.binaries))?;
    aggregate_mapping_is_valid(&raw, &binaries, source_root).then_some(())?;
    let aggregate = ValidatedCheckAggregate {
        input_fingerprint: raw.input_fingerprint,
        generation_fingerprint: raw.generation_fingerprint,
        selection_context_fingerprint: raw.selection_context_fingerprint,
        ordinary_source_digests: raw.ordinary_source_digests,
        selectors: raw.selectors,
        selector_binary_ids: raw.selector_binary_ids,
        binaries,
        aggregate_covered_lines: raw.aggregate_covered_lines,
        integrity_fingerprint: raw.integrity_fingerprint,
    };
    (aggregate.integrity_fingerprint == integrity_fingerprint(&aggregate)).then_some(aggregate)
}

fn raw_identity_is_valid(
    raw: &OnDiskCheckAggregate,
    source_root: &Path,
    mode: &CheckAggregateLoadMode,
) -> bool {
    raw.schema_version == CHECK_AGGREGATE_SCHEMA_VERSION
        && raw.cache_schema_version == CACHE_SCHEMA_VERSION
        && raw.source_root == normalized_source_root(source_root)
        && raw.selection_context_fingerprint == expected_selection_context(mode)
        && current_mode_identity_is_valid(raw, mode)
}

fn expected_selection_context(mode: &CheckAggregateLoadMode) -> &str {
    match mode {
        CheckAggregateLoadMode::Current {
            selection_context_fingerprint,
            ..
        }
        | CheckAggregateLoadMode::ReusablePrior {
            selection_context_fingerprint,
        } => selection_context_fingerprint,
    }
}

fn current_mode_identity_is_valid(
    raw: &OnDiskCheckAggregate,
    mode: &CheckAggregateLoadMode,
) -> bool {
    match mode {
        CheckAggregateLoadMode::Current {
            input_fingerprint,
            generation_fingerprint,
            ..
        } => {
            raw.input_fingerprint == *input_fingerprint
                && raw.generation_fingerprint == *generation_fingerprint
        }
        CheckAggregateLoadMode::ReusablePrior { .. } => true,
    }
}

fn selector_population_is_valid(raw: &OnDiskCheckAggregate, selectors: Option<&[String]>) -> bool {
    is_sorted_unique_nonempty(&raw.selectors)
        && requested_selectors_match(&raw.selectors, selectors)
        && raw.selector_binary_ids.keys().cloned().collect::<Vec<_>>() == raw.selectors
        && raw
            .selector_binary_ids
            .values()
            .all(|binary_ids| is_sorted_unique_nonempty(binary_ids))
}

fn requested_selectors_match(stored: &[String], selectors: Option<&[String]>) -> bool {
    let Some(selectors) = selectors else {
        return true;
    };
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    stored == expected
}

fn validated_binary_records(
    source_root: &Path,
    records: Vec<CheckAggregateBinaryRecord>,
) -> Option<BTreeMap<String, CheckAggregateBinaryRecord>> {
    let mut binaries = BTreeMap::new();
    for record in records {
        binary_record_is_valid(source_root, &record, &binaries).then_some(())?;
        binaries.insert(record.id.clone(), record);
    }
    Some(binaries)
}

fn binary_record_is_valid(
    source_root: &Path,
    record: &CheckAggregateBinaryRecord,
    binaries: &BTreeMap<String, CheckAggregateBinaryRecord>,
) -> bool {
    !record.id.is_empty()
        && !binaries.contains_key(&record.id)
        && repo_relative_path(source_root, Path::new(&record.executable)).is_some()
        && !record.digest.is_empty()
        && validate_line_map(source_root, &record.line_map)
}

fn aggregate_mapping_is_valid(
    raw: &OnDiskCheckAggregate,
    binaries: &BTreeMap<String, CheckAggregateBinaryRecord>,
    source_root: &Path,
) -> bool {
    raw.selector_binary_ids
        .values()
        .flatten()
        .all(|binary_id| binaries.contains_key(binary_id))
        && validate_line_map(source_root, &raw.aggregate_covered_lines)
        && union_binary_maps(binaries.values().map(|record| &record.line_map))
            == raw.aggregate_covered_lines
}

pub fn build_check_aggregate(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    selector_binary_ids: BTreeMap<String, Vec<String>>,
    test_binaries: &[RustTestBinaryIdentity],
    binary_line_maps: BTreeMap<String, RustLineCoverage>,
) -> Result<ValidatedCheckAggregate, RustLlvmCovError> {
    let mut sorted_selectors = selectors.to_vec();
    sorted_selectors.sort();
    sorted_selectors.dedup();
    let binary_identity_by_id: BTreeMap<_, _> = test_binaries
        .iter()
        .map(|binary| (binary.id.clone(), binary))
        .collect();
    let mut binaries = BTreeMap::new();
    for binary_id in selector_binary_ids.values().flatten() {
        if binaries.contains_key(binary_id) {
            continue;
        }
        let binary = binary_identity_by_id.get(binary_id).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing test-binary identity for aggregate binary `{binary_id}`"
            ))
        })?;
        let coverage = binary_line_maps.get(binary_id).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing line map for aggregate binary `{binary_id}`"
            ))
        })?;
        binaries.insert(
            binary_id.clone(),
            CheckAggregateBinaryRecord {
                id: binary.id.clone(),
                executable: binary.executable.clone(),
                digest: binary.digest.clone(),
                line_map: normalize_coverage_map(&req.source_root, coverage)?,
            },
        );
    }
    let aggregate_covered_lines =
        union_binary_maps(binaries.values().map(|record| &record.line_map));
    let mut aggregate = ValidatedCheckAggregate {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        ordinary_source_digests: identity.ordinary_source_digests.clone(),
        selectors: sorted_selectors,
        selector_binary_ids,
        binaries,
        aggregate_covered_lines,
        integrity_fingerprint: String::new(),
    };
    aggregate.integrity_fingerprint = integrity_fingerprint(&aggregate);
    let raw = on_disk_from_validated(req, &aggregate);
    validate_check_aggregate(
        raw,
        &req.source_root,
        Some(&aggregate.selectors),
        CheckAggregateLoadMode::Current {
            input_fingerprint: identity.input_digest.clone(),
            generation_fingerprint: identity.generation_fingerprint.clone(),
            selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        },
    )
    .ok_or_else(|| RustLlvmCovError::InvalidRequest("built invalid check aggregate".into()))
}

pub fn publish_check_aggregate(
    req: &RustCoverageBatchRequest,
    aggregate: &ValidatedCheckAggregate,
) -> Result<(), RustLlvmCovError> {
    let path = check_aggregate_path(&req.cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("aggregate path has no parent".into()))?;
    let tmp = parent.join(format!(
        ".check_aggregate.{}.tmp",
        crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_unique_suffix()
    ));
    let raw = on_disk_from_validated(req, aggregate);
    crate::kiss_publication_barrier::publish_atomically("rust_check_aggregate", &path, &tmp, |file| {
        serde_json::to_writer(&mut *file, &raw).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        Ok(())
    })
    .map_err(RustLlvmCovError::Io)
}

fn on_disk_from_validated(
    req: &RustCoverageBatchRequest,
    aggregate: &ValidatedCheckAggregate,
) -> OnDiskCheckAggregate {
    OnDiskCheckAggregate {
        schema_version: CHECK_AGGREGATE_SCHEMA_VERSION.to_string(),
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        source_root: normalized_source_root(&req.source_root),
        input_fingerprint: aggregate.input_fingerprint.clone(),
        generation_fingerprint: aggregate.generation_fingerprint.clone(),
        selection_context_fingerprint: aggregate.selection_context_fingerprint.clone(),
        ordinary_source_digests: aggregate.ordinary_source_digests.clone(),
        selectors: aggregate.selectors.clone(),
        selector_binary_ids: aggregate.selector_binary_ids.clone(),
        binaries: aggregate.binaries.values().cloned().collect(),
        aggregate_covered_lines: aggregate.aggregate_covered_lines.clone(),
        integrity_fingerprint: aggregate.integrity_fingerprint.clone(),
    }
}

fn check_aggregate_path(cache_root: &Path) -> PathBuf {
    cache_root.join("check_aggregate.json")
}

pub(crate) fn selector_binary_ids_from_outcomes(
    outcomes: &[crate::rust_llvm_cov_runner::RustLlvmCovOutcome],
) -> BTreeMap<String, Vec<String>> {
    outcomes
        .iter()
        .map(|outcome| (outcome.selector.clone(), outcome.test_binary_ids.clone()))
        .collect()
}

pub fn reusable_check_aggregate_delta(
    source_root: &Path,
    prior: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> RustSnapshotDelta {
    crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::reusable_snapshot_delta(
        source_root,
        prior,
        current,
    )
}

fn normalize_coverage_map(
    source_root: &Path,
    coverage: &RustLineCoverage,
) -> Result<BTreeMap<String, BTreeSet<u32>>, RustLlvmCovError> {
    let mut files = BTreeMap::new();
    for (file, lines) in &coverage.files {
        let rel = repo_relative_coverage_file(source_root, file).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "aggregate coverage path is outside repository Rust sources: {file}"
            ))
        })?;
        if lines.iter().any(|line| *line == 0) {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "aggregate coverage path `{rel}` contains non-positive line"
            )));
        }
        files.entry(rel).or_insert_with(BTreeSet::new).extend(lines);
    }
    Ok(files)
}

fn is_sorted_unique_nonempty(values: &[String]) -> bool {
    !values.is_empty() && values.windows(2).all(|window| window[0] < window[1])
}

#[cfg(test)]
#[path = "batch_check_aggregate_test.rs"]
mod tests;
