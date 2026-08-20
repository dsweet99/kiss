use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::CACHE_SCHEMA_VERSION;
use crate::publish_derived::batch_check_aggregate::{
    CHECK_AGGREGATE_SCHEMA_VERSION, ValidatedCheckAggregate,
};
use crate::rust_cov_cache::repo_relative_coverage_file;

pub(crate) fn validate_line_map(source_root: &Path, map: &BTreeMap<String, BTreeSet<u32>>) -> bool {
    map.iter().all(|(path, lines)| {
        !lines.is_empty()
            && lines.iter().all(|line| *line > 0)
            && !Path::new(path).is_absolute()
            && repo_relative_coverage_file(source_root, path).as_deref() == Some(path.as_str())
    })
}

pub(crate) fn validate_ordinary_source_digests(digests: &BTreeMap<String, String>) -> bool {
    digests.iter().all(|(path, digest)| {
        let parsed = Path::new(path);
        !path.is_empty()
            && !parsed.is_absolute()
            && !digest.is_empty()
            && path != "."
            && !path.contains('\\')
            && parsed.components().all(|component| {
                matches!(
                    component,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
    })
}

pub(crate) fn union_binary_maps<'a>(
    maps: impl Iterator<Item = &'a BTreeMap<String, BTreeSet<u32>>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut union = BTreeMap::<String, BTreeSet<u32>>::new();
    for map in maps {
        for (file, lines) in map {
            union.entry(file.clone()).or_default().extend(lines);
        }
    }
    union
}

pub(crate) fn integrity_fingerprint(aggregate: &ValidatedCheckAggregate) -> String {
    let mut h = crate::rust_cov_cache::rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        CHECK_AGGREGATE_SCHEMA_VERSION.as_bytes(),
    );
    for value in [
        CACHE_SCHEMA_VERSION,
        &aggregate.input_fingerprint,
        &aggregate.generation_fingerprint,
        &aggregate.selection_context_fingerprint,
    ] {
        h = hash_str(h, value);
    }
    h = hash_string_map(h, &aggregate.ordinary_source_digests);
    h = hash_string_list(h, &aggregate.selectors);
    for (selector, binary_ids) in &aggregate.selector_binary_ids {
        h = hash_str(h, selector);
        h = hash_string_list(h, binary_ids);
    }
    for record in aggregate.binaries.values() {
        h = hash_str(h, &record.id);
        h = hash_str(h, &record.executable);
        h = hash_str(h, &record.digest);
        h = hash_line_map(h, &record.line_map);
    }
    h = hash_line_map(h, &aggregate.aggregate_covered_lines);
    format!("{h:016x}")
}

pub(crate) fn stable_check_aggregate_identity(aggregate: &ValidatedCheckAggregate) -> String {
    integrity_fingerprint(aggregate)
}

fn hash_string_map(mut h: u64, map: &BTreeMap<String, String>) -> u64 {
    for (key, value) in map {
        h = hash_str(h, key);
        h = hash_str(h, value);
    }
    h
}

fn hash_string_list(mut h: u64, values: &[String]) -> u64 {
    for value in values {
        h = hash_str(h, value);
    }
    h
}

fn hash_line_map(mut h: u64, map: &BTreeMap<String, BTreeSet<u32>>) -> u64 {
    for (file, lines) in map {
        h = hash_str(h, file);
        for line in lines {
            h = crate::rust_cov_cache::rust_cov_fnv1a64(h, line.to_le_bytes().as_slice());
        }
        h = crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    }
    h
}

fn hash_str(mut h: u64, value: &str) -> u64 {
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, value.as_bytes());
    crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0])
}
