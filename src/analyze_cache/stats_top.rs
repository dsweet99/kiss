use kiss::check_universe_cache::FullCheckCache;
use kiss::{Config, GateConfig};
use std::path::PathBuf;

use super::fingerprint_for_check;
use super::load_verified_full_cache;
use super::path_helpers::same_cached_paths;
use crate::analyze::FocusFilter;

pub(crate) fn try_run_cached_stats_summary(
    universe: &str,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let cache =
        load_top_compatible_cache(universe, py_files, rs_files, py_config, rs_config, gate_config)?;
    if cache.py_file_count > 0 && cache.py_stats.is_none() {
        return None;
    }
    if cache.rs_file_count > 0 && cache.rs_stats.is_none() {
        return None;
    }
    Some(cache)
}

fn load_top_compatible_cache(
    universe: &str,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let fp = fingerprint_for_check(py_files, rs_files, py_config, rs_config, gate_config);
    let repo_root = super::repo_root_for_universe(universe);
    let cache = load_verified_full_cache(&repo_root, &fp, py_files, rs_files)?;
    let focus = FocusFilter::unrestricted();
    if !same_cached_paths(py_files, rs_files, &focus, &cache) {
        return None;
    }
    Some(cache)
}

#[cfg(test)]
mod tests;
