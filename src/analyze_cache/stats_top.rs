use kiss::check_universe_cache::{CachedCoverageItem, FullCheckCache};
use kiss::{Config, GateConfig};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::fingerprint_for_check;
use super::path_helpers::{load_full_cache, same_cached_paths};
use super::{FullCacheInputs, store_full_cache_from_run};

pub(crate) fn try_run_cached_stats_summary(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let cache = load_top_compatible_cache(py_files, rs_files, py_config, rs_config, gate_config)?;
    if cache.py_file_count > 0 && cache.py_stats.is_none() {
        return None;
    }
    if cache.rs_file_count > 0 && cache.rs_stats.is_none() {
        return None;
    }
    Some(cache)
}

pub(crate) fn try_run_cached_stats_top(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<HashMap<PathBuf, usize>> {
    let cache =
        load_top_compatible_cache(py_files, rs_files, py_config, rs_config, gate_config).or_else(
            || load_top_only_cache(py_files, rs_files, py_config, rs_config, gate_config),
        )?;
    if cache.definitions.is_empty() && cache.unreferenced.is_empty() {
        return None;
    }
    let defs: Vec<PathBuf> = cache.definitions.iter().map(item_path_buf).collect();
    let unrefs: Vec<PathBuf> = cache.unreferenced.iter().map(item_path_buf).collect();
    Some(kiss::cli_output::file_coverage_map_from_paths(
        &defs, &unrefs,
    ))
}

fn item_path_buf(item: &CachedCoverageItem) -> PathBuf {
    PathBuf::from(&item.file)
}

fn top_only_fingerprint(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> String {
    format!(
        "stats_top_{}",
        fingerprint_for_check(py_files, rs_files, py_config, rs_config, gate_config)
    )
}

fn load_top_only_cache(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let fp = top_only_fingerprint(py_files, rs_files, py_config, rs_config, gate_config);
    let cache = load_full_cache(&fp)?;
    let focus_set: HashSet<PathBuf> = py_files.iter().chain(rs_files).cloned().collect();
    if !same_cached_paths(py_files, rs_files, &focus_set, &cache) {
        return None;
    }
    Some(cache)
}

pub(crate) fn maybe_store_stats_top_cache(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
    definitions: Vec<CachedCoverageItem>,
    unreferenced: Vec<CachedCoverageItem>,
) {
    let fp = top_only_fingerprint(py_files, rs_files, py_config, rs_config, gate_config);
    if load_full_cache(&fp).is_some() {
        return;
    }
    let mut focus_paths: Vec<String> = py_files
        .iter()
        .chain(rs_files)
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    focus_paths.sort();
    store_full_cache_from_run(FullCacheInputs {
        fingerprint: fp,
        py_file_count: py_files.len(),
        rs_file_count: rs_files.len(),
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        coverage_violations: &[],
        py_graph: None,
        rs_graph: None,
        py_stats: None,
        rs_stats: None,
        focus_paths,
        py_paths: py_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        rs_paths: rs_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        py_dups_all: &[],
        rs_dups_all: &[],
        definitions,
        unreferenced,
    });
}

fn load_top_compatible_cache(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let fp = fingerprint_for_check(py_files, rs_files, py_config, rs_config, gate_config);
    let cache = load_full_cache(&fp)?;
    let focus_set: HashSet<PathBuf> = py_files.iter().chain(rs_files).cloned().collect();
    if !same_cached_paths(py_files, rs_files, &focus_set, &cache) {
        return None;
    }
    Some(cache)
}

#[cfg(test)]
mod tests;
