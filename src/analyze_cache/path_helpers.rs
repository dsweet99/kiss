use kiss::check_cache;
use kiss::check_universe_cache::FullCheckCache;
use std::path::PathBuf;

use crate::analyze::FocusFilter;

pub(super) fn cache_path_full(fingerprint: &str) -> PathBuf {
    check_cache::cache_dir().join(format!("check_full_{fingerprint}.bin"))
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

pub(super) fn load_full_cache(fingerprint: &str) -> Option<FullCheckCache> {
    let p = cache_path_full(fingerprint);
    let bytes = std::fs::read(p).ok()?;
    let c: FullCheckCache = bincode::deserialize(&bytes).ok()?;
    (c.fingerprint == fingerprint).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze_cache::test_helpers::empty_cache;
    use std::collections::HashSet;

    #[test]
    fn empty_cache_paths_accept_first_run_without_path_snapshot() {
        let cache = empty_cache("fp");
        let focus = FocusFilter::unrestricted();

        assert!(same_cached_paths(
            &[PathBuf::from("/repo/a.py")],
            &[PathBuf::from("/repo/src/lib.rs")],
            &focus,
            &cache,
        ));
    }

    #[test]
    fn cached_paths_are_order_insensitive_and_focus_sensitive() {
        let mut cache = empty_cache("fp");
        cache.py_paths = vec!["/repo/b.py".to_string(), "/repo/a.py".to_string()];
        cache.rs_paths = vec!["/repo/src/lib.rs".to_string()];
        cache.focus_restrict = true;
        cache.focus_paths = vec!["/repo/a.py".to_string()];
        let current_py = vec![PathBuf::from("/repo/a.py"), PathBuf::from("/repo/b.py")];
        let current_rs = vec![PathBuf::from("/repo/src/lib.rs")];
        let focus = FocusFilter::restricting(HashSet::from([PathBuf::from("/repo/a.py")]));

        assert!(same_cached_paths(&current_py, &current_rs, &focus, &cache));

        let other_focus = FocusFilter::restricting(HashSet::from([PathBuf::from("/repo/b.py")]));
        assert!(!same_cached_paths(
            &current_py,
            &current_rs,
            &other_focus,
            &cache,
        ));
    }

    #[test]
    fn cached_path_mismatch_rejects_replay() {
        let mut cache = empty_cache("fp");
        cache.py_paths = vec!["/repo/a.py".to_string()];
        let focus = FocusFilter::unrestricted();

        assert!(!same_cached_paths(
            &[PathBuf::from("/repo/a.py"), PathBuf::from("/repo/b.py")],
            &[],
            &focus,
            &cache,
        ));
    }
}
