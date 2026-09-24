use std::collections::BTreeSet;
use std::path::Path;

use kiss::Language;

use super::types::TargetRequest;

pub(crate) fn manifests_complete(repo_root: &Path, request: &TargetRequest) -> bool {
    let (need_python, need_rust) = needed_langs(repo_root, request);
    if need_python
        && python_inventory_plan(repo_root).is_none()
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
    if need_python {
        let Some(plan) = python_inventory_plan(repo_root) else {
            return cached_python.is_empty() && !has_python_test_files(repo_root, request);
        };
        if !same_set(cached_python, &plan.selectors) {
            return false;
        }
        let Ok(fingerprint) =
            crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(
                repo_root,
            )
        else {
            return false;
        };
        if plan.base_identity.input_fingerprint != fingerprint {
            return false;
        }
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
    let lang = request.lang.map(|filter| filter.to_language());
    let root = repo_root.to_string_lossy().into_owned();
    let (python, rust) =
        kiss::gather_files_by_lang(std::slice::from_ref(&root), lang, &request.ignore);
    let need_python = lang != Some(Language::Rust) && !python.is_empty();
    let need_rust = lang != Some(Language::Python) && !rust.is_empty();
    (need_python, need_rust)
}

pub(crate) fn has_python_test_files(repo_root: &Path, request: &TargetRequest) -> bool {
    let lang = request.lang.map(|filter| filter.to_language());
    let root = repo_root.to_string_lossy().into_owned();
    let (python, _) =
        kiss::gather_files_by_lang(std::slice::from_ref(&root), lang, &request.ignore);
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

fn python_inventory_plan(
    repo_root: &Path,
) -> Option<crate::test_runner::lang_python::generation::PythonPopulationPlan> {
    crate::test_runner::lang_python::generation::try_load_complete_pinned_python_plan(repo_root)
}

fn rust_inventory_selectors(repo_root: &Path) -> Option<Vec<String>> {
    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    crate::test_runner::execution_generation::load_current_generation(&cache)
        .ok()
        .map(|(generation, _)| generation.selectors)
}

fn same_set(left: &[String], right: &[String]) -> bool {
    let a: BTreeSet<&str> = left.iter().map(String::as_str).collect();
    let b: BTreeSet<&str> = right.iter().map(String::as_str).collect();
    a == b
}
