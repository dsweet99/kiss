use kiss::check_cache;
use kiss::check_universe_cache::FullCheckCache;
use std::path::{Path, PathBuf};

use crate::analyze::FocusFilter;

pub(super) fn cache_path_full(repo_root: &Path, fingerprint: &str) -> PathBuf {
    check_cache::cache_dir(repo_root).join(format!("check_full_{fingerprint}.bin"))
}

pub(super) fn same_cached_paths(
    current_py: &[PathBuf],
    current_rs: &[PathBuf],
    focus: &FocusFilter,
    cache: &FullCheckCache,
) -> bool {
    if cache.py_paths.is_empty() && cache.rs_paths.is_empty() {
        return true;
    }
    if cache.py_paths.len() != current_py.len() || cache.rs_paths.len() != current_rs.len() {
        return false;
    }
    let mut cache_py = cache.py_paths.clone();
    let mut cache_rs = cache.rs_paths.clone();
    let mut current_py: Vec<String> = current_py
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let mut current_rs: Vec<String> = current_rs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    cache_py.sort();
    cache_rs.sort();
    current_py.sort();
    current_rs.sort();
    if cache_py != current_py || cache_rs != current_rs {
        return false;
    }

    if cache.focus_restrict != focus.is_active() {
        return false;
    }

    if !cache.focus_restrict {
        return true;
    }

    let mut cache_focus = cache.focus_paths.clone();
    cache_focus.sort();
    focus.cache_focus_paths() == cache_focus
}

pub(super) fn load_full_cache(repo_root: &Path, fingerprint: &str) -> Option<FullCheckCache> {
    let p = cache_path_full(repo_root, fingerprint);
    let bytes = std::fs::read(p).ok()?;
    let c: FullCheckCache = bincode::deserialize(&bytes).ok()?;
    (c.fingerprint == fingerprint).then_some(c)
}
