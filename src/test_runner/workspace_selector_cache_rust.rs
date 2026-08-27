use std::path::Path;
use std::sync::Mutex;

type RustSelectorMemo = (String, Vec<String>, String, Vec<String>);
static RUST_SELECTOR_MEMO: Mutex<Option<RustSelectorMemo>> = Mutex::new(None);

fn rust_cache_matches(
    cache: &super::WorkspaceSelectorCache,
    repo_root: &Path,
    ignore: &[String],
    rust_fp: &str,
) -> bool {
    super::cache_identity_matches(cache, repo_root, ignore)
        && cache.rust_files_fingerprint == rust_fp
}

pub(super) fn remember_rust_selectors(
    root: &str,
    ignore: &[String],
    rust_fp: &str,
    selectors: &[String],
) {
    let Ok(mut memo) = RUST_SELECTOR_MEMO.lock() else {
        return;
    };
    *memo = Some((
        root.to_string(),
        ignore.to_vec(),
        rust_fp.to_string(),
        selectors.to_vec(),
    ));
}

fn recall_rust_selectors(root: &str, ignore: &[String], rust_fp: &str) -> Option<Vec<String>> {
    let memo = RUST_SELECTOR_MEMO.lock().ok()?;
    let (memo_root, memo_ignore, memo_fp, selectors) = memo.as_ref()?;
    (memo_root == root && memo_ignore == ignore && memo_fp == rust_fp).then(|| selectors.clone())
}

#[cfg(test)]
pub(crate) fn clear_rust_selector_memo_for_tests() {
    if let Ok(mut memo) = RUST_SELECTOR_MEMO.lock() {
        *memo = None;
    }
}

fn read_any_cache(repo_root: &Path) -> Option<super::WorkspaceSelectorCache> {
    super::read_cache_at(&super::cache_path(repo_root))
        .or_else(|| super::read_cache_at(&super::durable_cache_path(repo_root)))
}

pub(crate) fn load_cached_rust_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Option<Vec<String>> {
    let fps = super::workspace_lang_fingerprints(repo_root, ignore).ok()?;
    let root = super::normalized_root(repo_root);
    if let Some(selectors) = recall_rust_selectors(&root, ignore, &fps.rust) {
        return Some(selectors);
    }
    let cache = read_any_cache(repo_root)?;
    if !rust_cache_matches(&cache, repo_root, ignore, &fps.rust) {
        return None;
    }
    remember_rust_selectors(&root, ignore, &fps.rust, &cache.rust_selectors);
    Some(cache.rust_selectors)
}

pub(crate) fn store_rust_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    rust_selectors: &[String],
) -> bool {
    let Ok(fps) = super::workspace_lang_fingerprints(repo_root, ignore) else {
        return false;
    };
    let root = super::normalized_root(repo_root);
    let existing = read_any_cache(repo_root)
        .filter(|cache| super::cache_identity_matches(cache, repo_root, ignore));
    let (python_files_fingerprint, python_selectors) = match existing {
        Some(cache) => (cache.python_files_fingerprint, cache.python_selectors),
        None => ("unpopulated".to_string(), Vec::new()),
    };
    let cache = super::WorkspaceSelectorCache {
        schema_version: super::SCHEMA_VERSION.to_string(),
        source_root: root.clone(),
        ignore: ignore.to_vec(),
        python_files_fingerprint,
        rust_files_fingerprint: fps.rust.clone(),
        python_selectors,
        rust_selectors: rust_selectors.to_vec(),
    };
    remember_rust_selectors(&root, ignore, &fps.rust, rust_selectors);
    super::persist_selector_cache(repo_root, &cache)
}
