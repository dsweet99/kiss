use std::path::Path;
use std::sync::Mutex;

type RustSelectorMemo = (String, Vec<String>, String, Vec<String>);
static RUST_SELECTOR_MEMO: Mutex<Option<RustSelectorMemo>> = Mutex::new(None);

fn rust_cache_matches(
    cache: &super::LanguageSelectorCache,
    repo_root: &Path,
    ignore: &[String],
    rust_fp: &str,
) -> bool {
    super::language_cache_matches(cache, repo_root, ignore, &[], &[], rust_fp)
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

fn read_any_rust_cache(repo_root: &Path) -> Option<super::LanguageSelectorCache> {
    super::read_language_cache(repo_root, super::RUST_CACHE_FILE)
}

pub(crate) fn cached_rust_selectors_if_rust_fingerprint_current(
    repo_root: &Path,
) -> Option<Vec<String>> {
    let cache = read_any_rust_cache(repo_root)?;
    if cache.source_root != super::normalized_root(repo_root) {
        return None;
    }
    let fps = super::workspace_lang_fingerprints(repo_root, &cache.ignore).ok()?;
    (cache.files_fingerprint == fps.rust).then_some(cache.selectors)
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
    let cache = read_any_rust_cache(repo_root)?;
    if !rust_cache_matches(&cache, repo_root, ignore, &fps.rust) {
        return None;
    }
    remember_rust_selectors(&root, ignore, &fps.rust, &cache.selectors);
    Some(cache.selectors)
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
    let existing = read_any_rust_cache(repo_root);
    if let Some(cache) = existing.as_ref()
        && !super::cache_identity_matches(cache, repo_root, ignore, &[], &[])
    {
        remember_rust_selectors(&root, ignore, &fps.rust, rust_selectors);
        return true;
    }
    let cache = super::language_cache(
        root.clone(),
        ignore,
        "rust",
        fps.rust.clone(),
        rust_selectors,
        &[],
    );
    remember_rust_selectors(&root, ignore, &fps.rust, rust_selectors);
    super::persist_selector_cache(repo_root, super::RUST_CACHE_FILE, &cache)
}
