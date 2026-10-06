use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kiss::rpytest_runner::TestStatus;
use kiss::rslip::LineCoverage;

use crate::test_runner::line_selection;

pub(crate) const POPULATION_SCHEMA_VERSION: &str = "rslip-python-population-v1";
pub(crate) const PYTHON_SELECTOR_DISCOVERY_VERSION: &str = "python-selector-discovery-v2";

pub(crate) mod manifest;
pub(crate) use manifest::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_environment_mismatch,
    python_population_manifest_is_current_for_args_with_env_keys, stored_python_universe_selectors,
};
#[cfg(test)]
pub(crate) use manifest::{
    PythonPopulationManifestIdentity, python_population_manifest_is_current_with_identity,
    read_python_population_manifest, write_python_population_manifest_for_args,
    write_python_population_manifest_with_identity,
};

pub(crate) mod population_durations;
pub(crate) use population_durations::load_current_python_population_durations;

pub(crate) mod storage;
#[cfg(test)]
pub(crate) use storage::{
    create_new_python_file, is_kiss_rslip_cache_dir, normalized_python_repo_root,
    python_coverage_entry_paths, python_entries_fingerprint, python_fnv1a64,
    python_repo_relative_coverage_file, python_repo_relative_path, python_unique_suffix,
};
pub(crate) use storage::{
    load_current_python_coverage_index, python_coverage_cache_root,
    python_coverage_index_file_present,
    python_repo_relative_coverage_file as repo_relative_coverage_file,
    python_repo_relative_path as repo_relative_path,
};

pub(crate) type PythonCoverageIndex = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn select_python_source_selectors_from_index(
    repo_root: &Path,
    source_paths: &[PathBuf],
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_python_coverage_index(repo_root)?;
    python_selectors_for_source_paths(repo_root, source_paths, &index)
}

pub(crate) fn select_python_source_selectors_hybrid(
    repo_root: &Path,
    source_paths: &[PathBuf],
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_python_coverage_index(repo_root)?;
    let changed_rels = python_changed_line_rels(repo_root, changed_lines);
    let line_selectors_by_file = python_selectors_by_changed_file_line(repo_root, &changed_rels);
    let mut selectors = BTreeSet::new();
    for source_path in source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        let Some(file_selectors) = index.get(&rel).filter(|selectors| !selectors.is_empty()) else {
            continue;
        };
        let selected_for_file = line_selectors_by_file
            .get(&rel)
            .filter(|selectors| !selectors.is_empty())
            .cloned()
            .unwrap_or_else(|| file_selectors.clone());
        selectors.extend(selected_for_file);
    }
    Some(selectors)
}

pub(crate) fn python_index_covers_source_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_args: &[String],
) -> bool {
    let _ = test_args;
    let Some(index) = load_current_python_coverage_index(repo_root) else {
        return false;
    };
    source_paths
        .iter()
        .all(|source_path| index_covers_source_path(repo_root, source_path, &index))
}

fn index_covers_source_path(
    repo_root: &Path,
    source_path: &Path,
    index: &PythonCoverageIndex,
) -> bool {
    repo_relative_path(repo_root, source_path).is_some_and(|rel| {
        index
            .get(&rel)
            .is_some_and(|selectors| !selectors.is_empty())
    })
}

pub(crate) fn load_python_entry_for_index(
    path: &Path,
) -> Option<(String, TestStatus, LineCoverage)> {
    let record = kiss::test_records::read_record(path)?;
    Some((
        record.test_id,
        record.status,
        LineCoverage {
            files: record.covered,
        },
    ))
}

pub(crate) fn python_selectors_for_source_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    index: &PythonCoverageIndex,
) -> Option<BTreeSet<String>> {
    let mut selectors = BTreeSet::new();
    for source_path in source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        let Some(file_selectors) = index.get(&rel).filter(|selectors| !selectors.is_empty()) else {
            continue;
        };
        selectors.extend(file_selectors.iter().cloned());
    }
    Some(selectors)
}

pub(crate) fn python_changed_line_rels(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    line_selection::changed_line_rels(repo_root, changed_lines, repo_relative_path)
}

pub(crate) fn python_selectors_by_changed_file_line(
    repo_root: &Path,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<String>> {
    if changed_rels.is_empty() {
        return BTreeMap::new();
    }
    let entries = load_python_entries_for_line_selection(repo_root);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (selector, coverage) in entries {
        for (file, covered_lines) in coverage.files {
            let Some(rel) = repo_relative_coverage_file(repo_root, &file) else {
                continue;
            };
            let Some(wanted_lines) = changed_rels.get(&rel) else {
                continue;
            };
            if !wanted_lines.is_disjoint(&covered_lines) {
                out.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    out
}

pub(crate) fn load_python_entries_for_line_selection(
    repo_root: &Path,
) -> Vec<(String, LineCoverage)> {
    storage::python_coverage_entry_paths(repo_root)
        .into_iter()
        .filter_map(|entry_path| {
            let (selector, status, coverage) = load_python_entry_for_index(&entry_path)?;
            (status == TestStatus::Passed && !coverage.files.is_empty())
                .then_some((selector, coverage))
        })
        .collect()
}

#[cfg(test)]
#[path = "python_coverage_index_test.rs"]
mod external_tests;

#[cfg(test)]
#[path = "python_coverage_index_b_test.rs"]
mod external_b_tests;
