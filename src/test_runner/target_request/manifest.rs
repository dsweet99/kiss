use std::collections::BTreeSet;
use std::path::Path;

use super::types::TargetRequest;
use crate::test_runner::language_keyed::LanguageKeyed;

pub(crate) fn manifests_complete(repo_root: &Path, request: &TargetRequest) -> bool {
    let (need_python, need_rust) = needed_langs(repo_root, request);
    if need_python
        && python_inventory_selectors(repo_root, request).is_none()
        && has_python_test_files(repo_root, request)
    {
        return false;
    }
    if need_rust && rust_inventory_selectors(repo_root).is_none() {
        return false;
    }
    true
}

pub(crate) fn cache_matches_inventory(
    repo_root: &Path,
    request: &TargetRequest,
    cached_python: &[String],
    cached_rust: &[String],
) -> bool {
    let (need_python, need_rust) = needed_langs(repo_root, request);
    if need_python
        && python_inventory_selectors(repo_root, request).is_none()
        && (!cached_python.is_empty() || has_python_test_files(repo_root, request))
    {
        return false;
    }
    if need_rust {
        let Some(generation) = rust_inventory_selectors(repo_root) else {
            return false;
        };
        if !same_set(cached_rust, &generation) {
            return false;
        }
        if crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(
            repo_root,
            &request.ignore,
        )
        .is_err()
        {
            return false;
        }
    }
    true
}

pub(crate) fn needed_langs(repo_root: &Path, request: &TargetRequest) -> (bool, bool) {
    let lang = request.lang;
    let root = repo_root.to_string_lossy().into_owned();
    let (python, rust) =
        kiss::gather_files_by_lang_opts(std::slice::from_ref(&root), lang, &request.ignore, false);
    let allowed = LanguageKeyed::from_fn(|language| language.allowed_by(lang));
    let need_python = allowed.python && !python.is_empty();
    let need_rust = allowed.rust && !rust.is_empty();
    (need_python, need_rust)
}

pub(crate) fn population_count_need(
    repo_root: &Path,
    request: &TargetRequest,
) -> crate::test_runner::workspace_selector_cache::SelectorCountNeed {
    let (need_python, need_rust) = needed_langs(repo_root, request);
    crate::test_runner::workspace_selector_cache::SelectorCountNeed {
        python: need_python && has_python_test_files(repo_root, request),
        rust: need_rust,
    }
}

pub(crate) fn has_python_test_files(repo_root: &Path, request: &TargetRequest) -> bool {
    let lang = request.lang;
    let root = repo_root.to_string_lossy().into_owned();
    let (python, _) =
        kiss::gather_files_by_lang_opts(std::slice::from_ref(&root), lang, &request.ignore, false);
    python.iter().any(|path| {
        let rel = path
            .strip_prefix(repo_root)
            .map(|item| item.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
        let name = std::path::Path::new(&rel)
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        name.starts_with("test_") || name.ends_with("_test.py") || rel.contains("/tests/")
    })
}

fn python_inventory_selectors(repo_root: &Path, request: &TargetRequest) -> Option<Vec<String>> {
    crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors(
        repo_root,
        &request.ignore,
        &[],
    )
}

fn rust_inventory_selectors(repo_root: &Path) -> Option<Vec<String>> {
    crate::test_runner::lang_rust::try_load_rust_execution_witness(repo_root, &[])
        .ok()
        .map(|witness| witness.selectors)
}

fn same_set(left: &[String], right: &[String]) -> bool {
    let a: BTreeSet<&str> = left.iter().map(String::as_str).collect();
    let b: BTreeSet<&str> = right.iter().map(String::as_str).collect();
    a == b
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod tests;
