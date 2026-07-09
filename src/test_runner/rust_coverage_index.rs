use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use serde::Deserialize;

use crate::test_runner::line_selection;

pub(crate) const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v1";
pub(crate) const INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v1";
pub(crate) const POPULATION_SCHEMA_VERSION: &str = "rust-llvm-cov-population-v1";
pub(crate) const RUST_SELECTOR_DISCOVERY_VERSION: &str = "rust-selector-discovery-v1";

#[path = "rust_coverage_index/manifest.rs"]
mod manifest;
pub(crate) use manifest::{
    RUST_COVERAGE_ENV_KEYS, rust_population_manifest_is_current_for_args,
    write_rust_population_manifest_for_args,
};
#[cfg(test)]
pub(crate) use manifest::{
    RustPopulationManifest, RustPopulationManifestIdentity,
    rust_population_manifest_is_current_with_identity,
    write_rust_population_manifest_with_identity,
};
#[path = "rust_coverage_index/storage.rs"]
mod storage;
#[cfg(test)]
pub(crate) use storage::{
    command_failure_message, command_output_text, fnv1a64, fnv1a64_step, is_cargo_config_file_name,
    is_cargo_config_input_path, path_has_cargo_parent, rust_coverage_index_path,
};
pub(crate) use storage::{
    command_stdout, create_new_file, entries_fingerprint, load_current_rust_coverage_index,
    normalized_repo_root, repo_relative_coverage_file, repo_relative_path,
    rust_coverage_cache_root, rust_coverage_entry_paths, rust_population_manifest_path,
    unique_suffix, workspace_input_fingerprint, write_rust_coverage_index,
};

pub(crate) type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn rebuild_rust_coverage_index(repo_root: &Path) -> Result<RustCoverageIndex, String> {
    let index = build_rust_coverage_index(repo_root)?;
    write_rust_coverage_index(repo_root, &index)?;
    Ok(index)
}

pub(crate) fn select_rust_source_selectors_from_index(
    repo_root: &Path,
    source_paths: &[PathBuf],
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_rust_coverage_index(repo_root)?;
    selectors_for_source_paths(repo_root, source_paths, &index)
}

#[cfg(test)]
pub(crate) fn select_rust_source_selectors_for_changed_lines(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if changed_lines.is_empty() {
        return Some(BTreeSet::new());
    }
    let cache_root = rust_coverage_cache_root(repo_root);
    let entries = load_entries_for_line_selection(&cache_root);
    if entries.is_empty() {
        return None;
    }
    let mut selectors = BTreeSet::new();
    for (source_path, wanted_lines) in changed_lines {
        if wanted_lines.is_empty() {
            return None;
        }
        let rel = repo_relative_path(repo_root, source_path)?;
        let mut file_selectors = BTreeSet::new();
        for (selector, coverage) in &entries {
            for (file, covered_lines) in &coverage.files {
                if repo_relative_coverage_file(repo_root, file).as_deref() == Some(rel.as_str())
                    && !wanted_lines.is_disjoint(covered_lines)
                {
                    file_selectors.insert(selector.clone());
                    break;
                }
            }
        }
        if file_selectors.is_empty() {
            return None;
        }
        selectors.extend(file_selectors);
    }
    Some(selectors)
}

pub(crate) fn select_rust_source_selectors_hybrid(
    repo_root: &Path,
    source_paths: &[PathBuf],
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_rust_coverage_index(repo_root)?;
    let changed_rels = changed_line_rels(repo_root, changed_lines);
    let line_selectors_by_file = selectors_by_changed_file_line(repo_root, &changed_rels);
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

fn changed_line_rels(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    line_selection::changed_line_rels(repo_root, changed_lines, repo_relative_path)
}

fn selectors_by_changed_file_line(
    repo_root: &Path,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<String>> {
    if changed_rels.is_empty() {
        return BTreeMap::new();
    }
    let cache_root = rust_coverage_cache_root(repo_root);
    let entries = load_entries_for_line_selection(&cache_root);
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

pub(crate) fn selectors_for_source_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    index: &RustCoverageIndex,
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

fn load_entries_for_line_selection(cache_root: &Path) -> Vec<(String, RustLineCoverage)> {
    rust_coverage_entry_paths(cache_root)
        .into_iter()
        .filter_map(|entry_path| {
            let (selector, status, coverage) = load_entry_for_index(&entry_path)?;
            (status == TestStatus::Passed && !coverage.files.is_empty())
                .then_some((selector, coverage))
        })
        .collect()
}

fn build_rust_coverage_index(repo_root: &Path) -> Result<RustCoverageIndex, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let mut files: RustCoverageIndex = BTreeMap::new();
    for entry_path in rust_coverage_entry_paths(&cache_root) {
        let Some((selector, status, coverage)) = load_entry_for_index(&entry_path) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            if let Some(rel) = repo_relative_coverage_file(repo_root, file) {
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    Ok(files)
}

fn load_entry_for_index(path: &Path) -> Option<(String, TestStatus, RustLineCoverage)> {
    #[derive(Deserialize)]
    struct RustCovCacheEntryForIndex {
        schema_version: String,
        selector: String,
        status: TestStatus,
        coverage: RustLineCoverage,
    }

    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntryForIndex = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == CACHE_SCHEMA_VERSION).then_some((
        entry.selector,
        entry.status,
        entry.coverage,
    ))
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use super::{
        RustPopulationManifest, RustPopulationManifestIdentity, command_stdout, fnv1a64,
        is_cargo_config_input_path,
    };

    #[test]
    fn witness_manifest_identity_and_private_helpers() {
        let identity: RustPopulationManifestIdentity = RustPopulationManifestIdentity {
            cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
            selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
            rustc_version: "rustc".to_string(),
            cargo_version: "cargo".to_string(),
            cargo_llvm_cov_version: "llvm-cov".to_string(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
        };
        let manifest: RustPopulationManifest = RustPopulationManifest {
            schema_version: POPULATION_SCHEMA_VERSION.to_string(),
            cache_schema_version: identity.cache_schema_version.clone(),
            source_root: "root".to_string(),
            selector_discovery_version: identity.selector_discovery_version.clone(),
            rustc_version: identity.rustc_version.clone(),
            cargo_version: identity.cargo_version.clone(),
            cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
            cargo_args: identity.cargo_args.clone(),
            test_args: identity.test_args.clone(),
            env: identity.env.clone(),
            input_fingerprint: "input".to_string(),
            entries_fingerprint: "entries".to_string(),
            selectors: Vec::new(),
        };
        assert!(identity.has_tool_versions());
        assert_eq!(identity.tool_versions(), ["rustc", "cargo", "llvm-cov"]);
        assert!(identity.args_match(&[], &[]));
        assert!(manifest.matches_identity(&identity, "root"));
        assert!(manifest.matches_selectors(&[]));
        assert_eq!(manifest.cache_schema_version, CACHE_SCHEMA_VERSION);
        assert!(
            command_stdout(Path::new("/definitely/not/a/command"), &[], Path::new(".")).is_err()
        );
        assert_eq!(command_output_text(b" ok \n"), "ok");
        assert!(command_failure_message(Path::new("cmd"), b"bad").contains("cmd failed: bad"));
        assert!(is_cargo_config_input_path(Path::new(".cargo/config.toml")));
        assert!(path_has_cargo_parent(Path::new(".cargo/config")));
        assert!(is_cargo_config_file_name(Path::new("config.toml")));
        assert_ne!(fnv1a64(0xcbf2_9ce4_8422_2325, b"x"), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(123, &[]), 123);
        assert_eq!(
            fnv1a64_step(1, b'a', 3),
            (1 ^ u64::from(b'a')).wrapping_mul(3)
        );
    }
}

#[cfg(test)]
#[path = "rust_coverage_index_test.rs"]
mod tests;

#[cfg(test)]
#[path = "rust_coverage_index/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "rust_coverage_index_manifest_test.rs"]
mod manifest_tests;
