use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;
use rslip::LineCoverage;
use serde::Deserialize;

use crate::test_runner::line_selection;

pub(crate) const INDEX_SCHEMA_VERSION: &str = "rslip-python-index-v1";
pub(crate) const POPULATION_SCHEMA_VERSION: &str = "rslip-python-population-v1";
pub(crate) const PYTHON_SELECTOR_DISCOVERY_VERSION: &str = "python-selector-discovery-v2";

mod manifest;
pub(crate) use manifest::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_manifest_is_current_for_args_with_env_keys,
    stored_python_universe_selectors,
};
#[cfg(test)]
pub(crate) use manifest::{
    PythonPopulationManifestIdentity, python_population_manifest_is_current_with_identity,
    read_python_population_manifest, write_python_population_manifest_for_args,
    write_python_population_manifest_with_identity,
};

mod storage;
#[cfg(test)]
pub(crate) use storage::{
    create_new_python_file, is_kiss_rslip_cache_dir, normalized_python_repo_root,
    python_coverage_entry_paths, python_entries_fingerprint, python_fnv1a64,
    python_repo_relative_coverage_file, python_repo_relative_path, python_unique_suffix,
};
pub(crate) use storage::{
    load_current_python_coverage_index, python_coverage_cache_root,
    python_repo_relative_coverage_file as repo_relative_coverage_file,
    python_repo_relative_path as repo_relative_path,
};

pub(crate) type PythonCoverageIndex = BTreeMap<String, BTreeSet<String>>;

#[cfg(test)]
pub(crate) fn rebuild_python_coverage_index(
    repo_root: &Path,
) -> Result<PythonCoverageIndex, String> {
    publish_python_derived_state_with_filter(repo_root, None, &[], |path, repo_root| {
        repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    })
}

#[cfg(test)]
pub(crate) fn rebuild_python_coverage_index_with_filter(
    repo_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<PythonCoverageIndex, String> {
    publish_python_derived_state_with_filter(repo_root, None, &[], is_indexable)
}

pub(crate) fn publish_python_derived_state_with_filter(
    repo_root: &Path,
    population_selectors: Option<&[String]>,
    test_args: &[String],
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<PythonCoverageIndex, String> {
    let cache_root = python_coverage_cache_root(repo_root)?;
    let _guard = rslip::lock_rslip_derived_state(&cache_root).map_err(|err| err.to_string())?;
    let (index, entries_fingerprint) =
        build_stable_python_coverage_index(repo_root, &cache_root, is_indexable)?;
    storage::write_python_coverage_index_with_entries_fingerprint(
        repo_root,
        &index,
        &entries_fingerprint,
    )?;
    if let Some(selectors) = population_selectors {
        let identity = manifest::current_python_population_manifest_identity(repo_root, test_args)?;
        manifest::write_python_population_manifest_with_identity_and_entries_fingerprint(
            repo_root,
            selectors,
            &identity,
            &entries_fingerprint,
        )?;
    }
    Ok(index)
}

fn build_stable_python_coverage_index(
    repo_root: &Path,
    cache_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<(PythonCoverageIndex, String), String> {
    for _ in 0..3 {
        let before = storage::python_entries_fingerprint(cache_root).map_err(|e| e.to_string())?;
        let index = build_python_coverage_index_with_filter(repo_root, &is_indexable);
        let after = storage::python_entries_fingerprint(cache_root).map_err(|e| e.to_string())?;
        if before == after {
            return Ok((index, after));
        }
    }
    Err(
        "error: kiss test: Python rslip entries changed during derived-state publication"
            .to_string(),
    )
}

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

#[cfg(test)]
pub(crate) fn build_python_coverage_index(repo_root: &Path) -> PythonCoverageIndex {
    build_python_coverage_index_with_filter(repo_root, |path, repo_root| {
        repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    })
}

fn build_python_coverage_index_with_filter(
    repo_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> PythonCoverageIndex {
    let Ok(cache_root) = python_coverage_cache_root(repo_root) else {
        return BTreeMap::new();
    };
    let mut files: PythonCoverageIndex = BTreeMap::new();
    for entry_path in storage::python_coverage_entry_paths(&cache_root) {
        let Some((selector, status, coverage)) = load_python_entry_for_index(&entry_path) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            let path = Path::new(file);
            if is_indexable(path, repo_root) {
                let rel = repo_relative_coverage_file(repo_root, file)
                    .expect("indexable Python coverage path has repo-relative form");
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    files
}

pub(crate) fn load_python_entry_for_index(
    path: &Path,
) -> Option<(String, TestStatus, LineCoverage)> {
    #[derive(Deserialize)]
    struct RslipCacheEntryForIndex {
        schema_version: String,
        nodeid: String,
        status: TestStatus,
        coverage: LineCoverage,
    }

    let bytes = fs::read(path).ok()?;
    let entry: RslipCacheEntryForIndex = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == rslip::CACHE_SCHEMA_VERSION).then_some((
        entry.nodeid,
        entry.status,
        entry.coverage,
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
    let Ok(cache_root) = python_coverage_cache_root(repo_root) else {
        return BTreeMap::new();
    };
    let entries = load_python_entries_for_line_selection(&cache_root);
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
    cache_root: &Path,
) -> Vec<(String, LineCoverage)> {
    storage::python_coverage_entry_paths(cache_root)
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
