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

pub(crate) mod manifest;
pub(crate) use manifest::{
    PYTHON_COVERAGE_ENV_KEYS, StoredPythonPopulation, python_population_environment_mismatch,
    python_population_manifest_is_current_for_args_with_env_keys,
    stored_python_universe_population, stored_python_universe_selectors,
};
#[cfg(test)]
pub(crate) use manifest::{
    PythonPopulationManifestIdentity, python_population_manifest_is_current_with_identity,
    read_python_population_manifest, write_python_population_manifest_for_args,
    write_python_population_manifest_with_identity,
};

pub(crate) mod population_durations;
pub(crate) use population_durations::{
    load_current_python_population_durations, load_current_python_population_max_duration,
    load_current_python_population_path_maxes,
};
#[cfg(test)]
pub(crate) use population_durations::write_population_durations;

pub(crate) mod coverage_snapshot;
pub(crate) use coverage_snapshot::{
    try_load_python_coverage_snapshot, write_python_coverage_snapshot,
};

pub(crate) mod storage;
#[cfg(test)]
pub(crate) use storage::{
    create_new_python_file, is_kiss_rslip_cache_dir, normalized_python_repo_root,
    python_coverage_entry_paths, python_entries_fingerprint, python_fnv1a64,
    python_repo_relative_coverage_file, python_repo_relative_path, python_unique_suffix,
    write_python_coverage_index_with_entries_fingerprint,
};
pub(crate) use storage::{
    load_current_python_coverage_index, python_coverage_cache_root,
    python_coverage_index_file_present,
    python_repo_relative_coverage_file as repo_relative_coverage_file,
    python_repo_relative_path as repo_relative_path,
};

pub(crate) use crate::test_runner::lang_python::generation;
pub(crate) use generation::{
    GenerationReason, clear_python_generation_warm_memo, current_complete_generation_matches,
    current_python_execution_identity, problem_selectors_from_timings,
    repair_python_population_generation, selector_deltas_from_cached_outcomes,
    try_load_pinned_python_generation, try_load_pinned_python_generation_warm,
    try_migrate_complete_v1_generation,
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
    publish_python_derived_state_with_filter_force(
        repo_root,
        population_selectors,
        test_args,
        false,
        is_indexable,
    )
}

pub(crate) fn publish_python_derived_state_with_filter_force(
    repo_root: &Path,
    population_selectors: Option<&[String]>,
    test_args: &[String],
    force_publish: bool,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<PythonCoverageIndex, String> {
    let selectors_to_publish = population_selectors
        .map(|selectors| selectors.to_vec())
        .or_else(|| {
            manifest::stored_python_universe_selectors_for_current_inputs(
                repo_root,
                test_args,
                manifest::PYTHON_COVERAGE_ENV_KEYS,
            )
        });
    let Some(selectors) = selectors_to_publish else {
        // Selective index-only rebuild when no population selectors are known.
        return rebuild_selection_index_fallback(repo_root, &is_indexable);
    };
    if !force_publish
        && generation::current_complete_generation_matches(repo_root, &selectors, test_args)
        && let Some(index) = load_current_python_coverage_index(repo_root)
    {
        return Ok(index);
    }
    if !force_publish
        && let Some(pinned) =
            generation::current_generation_matches_plan(repo_root, &selectors, test_args)
    {
        if !pinned.complete {
            let problems = generation::problem_selectors_from_timings(&pinned.timings);
            let deltas = generation::selector_deltas_from_cached_outcomes(
                repo_root,
                &problems,
                test_args,
                &is_indexable,
                &kiss::GateConfig::load(),
            )?;
            let _ = generation::repair_python_population_generation(
                repo_root,
                &deltas,
                GenerationReason::IncompleteRepair,
            )?;
        }
        return load_current_python_coverage_index(repo_root).ok_or_else(|| {
            "error: kiss test: Python generation index missing after incomplete repair".to_string()
        });
    }
    if !force_publish
        && let Ok(Some(_)) =
            generation::try_migrate_complete_v1_generation(repo_root, test_args, &is_indexable)
    {
        return load_current_python_coverage_index(repo_root).ok_or_else(|| {
            "error: kiss test: Python generation index missing after v1 migration".to_string()
        });
    }
    let reason = if force_publish {
        GenerationReason::CompleteForce
    } else if population_selectors.is_some() {
        GenerationReason::Complete
    } else {
        GenerationReason::ColdCov
    };
    let (_plan, _id) = generation::materialize_and_publish_from_cached_outcomes(
        repo_root,
        &selectors,
        test_args,
        reason,
        &is_indexable,
    )?;
    load_current_python_coverage_index(repo_root).ok_or_else(|| {
        "error: kiss test: Python generation index missing after publication".to_string()
    })
}

fn rebuild_selection_index_fallback(
    repo_root: &Path,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> Result<PythonCoverageIndex, String> {
    let cache_root = python_coverage_cache_root(repo_root)?;
    let _guard = rslip::lock_rslip_derived_state(&cache_root).map_err(|err| err.to_string())?;
    let (index, _covered_lines, entries_fingerprint) =
        build_stable_python_coverage_index(repo_root, &cache_root, is_indexable)?;
    storage::write_python_coverage_index_with_entries_fingerprint(
        repo_root,
        &index,
        &entries_fingerprint,
    )?;
    Ok(index)
}

fn build_stable_python_coverage_index(
    repo_root: &Path,
    cache_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<(PythonCoverageIndex, coverage_snapshot::CoveredLinesMap, String), String> {
    for _ in 0..3 {
        let before = storage::python_entries_fingerprint(cache_root).map_err(|e| e.to_string())?;
        let (index, covered_lines) =
            build_python_coverage_index_and_lines(repo_root, &is_indexable);
        let after = storage::python_entries_fingerprint(cache_root).map_err(|e| e.to_string())?;
        if before == after {
            return Ok((index, covered_lines, after));
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
    build_python_coverage_index_and_lines(repo_root, |path, repo_root| {
        repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    })
    .0
}

fn build_python_coverage_index_and_lines(
    repo_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> (PythonCoverageIndex, coverage_snapshot::CoveredLinesMap) {
    let Ok(cache_root) = python_coverage_cache_root(repo_root) else {
        return (BTreeMap::new(), BTreeMap::new());
    };
    let mut files: PythonCoverageIndex = BTreeMap::new();
    let mut covered_lines: coverage_snapshot::CoveredLinesMap = BTreeMap::new();
    for entry_path in storage::python_coverage_entry_paths(&cache_root) {
        let Some((selector, status, coverage)) = load_python_entry_for_index(&entry_path) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for (file, lines) in coverage.files {
            let path = Path::new(&file);
            if is_indexable(path, repo_root) {
                let rel = repo_relative_coverage_file(repo_root, &file)
                    .expect("indexable Python coverage path has repo-relative form");
                files
                    .entry(rel.clone())
                    .or_default()
                    .insert(selector.clone());
                covered_lines.entry(rel).or_default().extend(lines);
            }
        }
    }
    (files, covered_lines)
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
    if let Ok(pinned) = generation::try_load_pinned_python_generation(repo_root) {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (rel, wanted_lines) in changed_rels {
            let Some(file_index) = pinned.line_index.get(rel) else {
                continue;
            };
            let mut selectors = BTreeSet::new();
            for line in wanted_lines {
                if let Some(ids) = file_index.get(line) {
                    selectors.extend(ids.iter().cloned());
                }
            }
            if !selectors.is_empty() {
                out.insert(rel.clone(), selectors);
            }
        }
        return out;
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

#[cfg(test)]
#[path = "python_coverage_index_b_test.rs"]
mod external_b_tests;
