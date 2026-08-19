use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{
    CheckAggregateBinaryRecord, OnDiskCheckAggregate, ValidatedCheckAggregate,
    check_aggregate_path, CHECK_AGGREGATE_SCHEMA_VERSION,
};
use crate::RustLineCoverage;

pub fn selector_coverage_from_check_aggregate_generation(
    cache_root: &Path,
    generation_fingerprint: &str,
) -> Option<BTreeMap<String, RustLineCoverage>> {
    let bytes = fs::read(check_aggregate_path(cache_root)).ok()?;
    let raw: OnDiskCheckAggregate = serde_json::from_slice(&bytes).ok()?;
    if raw.schema_version != CHECK_AGGREGATE_SCHEMA_VERSION
        || raw.generation_fingerprint != generation_fingerprint
    {
        return None;
    }
    Some(selector_coverage_from_on_disk(&raw))
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

fn selector_coverage_from_on_disk(
    raw: &OnDiskCheckAggregate,
) -> BTreeMap<String, RustLineCoverage> {
    let binaries: BTreeMap<&str, &CheckAggregateBinaryRecord> = raw
        .binaries
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect();
    let mut out = BTreeMap::new();
    for (selector, binary_ids) in &raw.selector_binary_ids {
        let mut files: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for binary_id in binary_ids {
            let Some(record) = binaries.get(binary_id.as_str()) else {
                continue;
            };
            for (path, lines) in &record.line_map {
                files
                    .entry(path.clone())
                    .or_default()
                    .extend(lines.iter().copied());
            }
        }
        out.insert(selector.clone(), RustLineCoverage { files });
    }
    out
}
