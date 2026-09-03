use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLineCoverage;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{
    CACHE_SCHEMA_VERSION, repo_relative_coverage_file, rust_coverage_cache_root,
    rust_coverage_entry_paths,
};

pub(crate) fn selectors_by_changed_file_line(
    repo_root: &Path,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
    generation_fingerprint: &str,
) -> BTreeMap<String, BTreeSet<String>> {
    if changed_rels.is_empty() {
        return BTreeMap::new();
    }
    let cache_root = rust_coverage_cache_root(repo_root);
    if let Some(from_reverse) = kiss::rust_llvm_cov_runner::query_reverse_line_index(
        &cache_root,
        generation_fingerprint,
        changed_rels,
    ) {
        return from_reverse;
    }
    if let Some(from_aggregate) =
        kiss::rust_llvm_cov_runner::selector_coverage_from_check_aggregate_generation(
            &cache_root,
            repo_root,
            generation_fingerprint,
        )
    {
        return selectors_from_coverage_maps(repo_root, changed_rels, from_aggregate);
    }
    let entries = load_entries_for_line_selection(&cache_root, generation_fingerprint);
    selectors_from_coverage_maps(repo_root, changed_rels, entries)
}

fn selectors_from_coverage_maps(
    repo_root: &Path,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
    maps: impl IntoIterator<Item = (String, RustLineCoverage)>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (selector, coverage) in maps {
        for (file, covered_lines) in coverage.files {
            let Some(rel) = repo_relative_coverage_file(repo_root, &file) else {
                continue;
            };
            let Some(wanted_lines) = changed_rels.get(&rel) else {
                continue;
            };
            if wanted_lines.is_disjoint(&covered_lines) {
                continue;
            }
            out.entry(rel).or_default().insert(selector.clone());
        }
    }
    out
}

pub(super) fn load_entries_for_line_selection(
    cache_root: &Path,
    generation_fingerprint: &str,
) -> Vec<(String, RustLineCoverage)> {
    rust_coverage_entry_paths(cache_root)
        .into_iter()
        .filter_map(|entry_path| {
            let (selector, status, coverage) =
                load_entry_for_line_selection(&entry_path, generation_fingerprint)?;
            (status == TestStatus::Passed && !coverage.files.is_empty())
                .then_some((selector, coverage))
        })
        .collect()
}

pub(super) fn load_entry_for_line_selection(
    path: &Path,
    generation_fingerprint: &str,
) -> Option<(String, TestStatus, RustLineCoverage)> {
    #[derive(Deserialize)]
    struct RustCovCacheEntryForIndex {
        schema_version: String,
        generation_fingerprint: String,
        selector: String,
        status: TestStatus,
        coverage: RustLineCoverage,
    }

    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntryForIndex = serde_json::from_slice(&bytes).ok()?;
    if entry.schema_version != CACHE_SCHEMA_VERSION {
        return None;
    }
    if !generation_fingerprint.is_empty() && entry.generation_fingerprint != generation_fingerprint
    {
        return None;
    }
    Some((entry.selector, entry.status, entry.coverage))
}
