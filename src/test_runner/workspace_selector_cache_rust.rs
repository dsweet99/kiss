use std::collections::{BTreeMap, BTreeSet};
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

fn read_rust_cache(repo_root: &Path, ignore: &[String]) -> Option<super::LanguageSelectorCache> {
    super::read_language_cache_for_identity(
        repo_root,
        super::RUST_CACHE_FILE,
        "rust",
        ignore,
        &[],
        &[],
    )
}

pub(crate) fn cached_rust_selectors_if_rust_fingerprint_current(
    repo_root: &Path,
) -> Option<Vec<String>> {
    let root = super::normalized_root(repo_root);
    let mut fingerprints: BTreeMap<Vec<String>, String> = BTreeMap::new();
    let mut known = BTreeSet::new();
    let mut found_current = false;
    for cache in super::read_all_language_caches(repo_root, super::RUST_CACHE_FILE) {
        if cache.language != "rust"
            || cache.source_root != root
            || !super::cache_identity_matches(&cache, repo_root, &cache.ignore, &[], &[])
        {
            continue;
        }
        let fingerprint = match fingerprints.get(&cache.ignore) {
            Some(fingerprint) => fingerprint.clone(),
            None => {
                let fingerprint = super::workspace_lang_fingerprints(repo_root, &cache.ignore)
                    .ok()?
                    .rust;
                fingerprints.insert(cache.ignore.clone(), fingerprint.clone());
                fingerprint
            }
        };
        if cache.files_fingerprint == fingerprint {
            found_current = true;
            known.extend(cache.selectors);
        }
    }
    found_current.then(|| known.into_iter().collect())
}

pub(crate) fn load_cached_rust_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Option<Vec<String>> {
    load_cached_rust_workspace_hit(repo_root, ignore).map(|(selectors, _)| selectors)
}

pub(crate) fn load_cached_rust_workspace_fingerprint(
    repo_root: &Path,
    ignore: &[String],
) -> Option<String> {
    load_cached_rust_workspace_hit(repo_root, ignore).map(|(_, fingerprint)| fingerprint)
}

pub(super) fn load_cached_rust_workspace_hit(
    repo_root: &Path,
    ignore: &[String],
) -> Option<(Vec<String>, String)> {
    let cache = read_rust_cache(repo_root, ignore);
    let fps = super::workspace_lang_fingerprints(repo_root, ignore).ok()?;
    let root = super::normalized_root(repo_root);
    if let Some(selectors) = recall_rust_selectors(&root, ignore, &fps.rust) {
        return Some((selectors, fps.rust));
    }
    let cache = cache.or_else(|| read_rust_cache(repo_root, ignore))?;
    if !rust_cache_matches(&cache, repo_root, ignore, &fps.rust) {
        return None;
    }
    remember_rust_selectors(&root, ignore, &fps.rust, &cache.selectors);
    Some((cache.selectors, fps.rust))
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
    let cache = super::language_cache(
        root.clone(),
        ignore,
        "rust",
        fps.rust.clone(),
        rust_selectors,
        &[],
    );
    remember_rust_selectors(&root, ignore, &fps.rust, rust_selectors);
    super::persist_selector_cache_for_identity(repo_root, super::RUST_CACHE_FILE, &cache)
}
