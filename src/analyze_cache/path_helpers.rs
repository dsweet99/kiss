use kiss::check_cache;
use kiss::check_universe_cache::FullCheckCache;
use std::collections::HashSet;
use std::path::PathBuf;

pub(super) fn cache_path_full(fingerprint: &str) -> PathBuf {
    check_cache::cache_dir().join(format!("check_full_{fingerprint}.bin"))
}

pub(super) fn same_cached_paths(
    current_py: &[PathBuf],
    current_rs: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
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

    let cache_focus = if cache.focus_paths.is_empty() {
        let mut inferred: Vec<String> = cache_py.clone();
        inferred.extend(cache_rs.iter().cloned());
        inferred.sort();
        inferred
    } else {
        let mut stored = cache.focus_paths.clone();
        stored.sort();
        stored
    };
    let mut current_focus: Vec<String> = focus_set
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    current_focus.sort();

    cache_focus == current_focus
}

pub(super) fn load_full_cache(fingerprint: &str) -> Option<FullCheckCache> {
    let p = cache_path_full(fingerprint);
    let bytes = std::fs::read(p).ok()?;
    let c: FullCheckCache = bincode::deserialize(&bytes).ok()?;
    (c.fingerprint == fingerprint).then_some(c)
}
