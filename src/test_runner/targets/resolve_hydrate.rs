use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::SourceModel;
use super::python_nodeid_cache::{
    lookup_python_file_nodeids, repo_relative, store_python_file_nodeids,
};
use crate::test_runner::runners::collect_python_nodeids_for_targets;
use crate::test_runner::workspace_selector_cache::python_selectors_for_rel_path;
use kiss::code_roles::is_python_test_module_path;

pub(super) fn python_nodeids_for_model(
    repo_root: &Path,
    model: &SourceModel,
    rel: &str,
    pytest_args: &[String],
    cached_selectors: Option<&[String]>,
) -> Result<Vec<String>, String> {
    if let Some(cached) = cached_selectors {
        let from_cache = python_selectors_for_rel_path(cached, rel);
        if !from_cache.is_empty() {
            return Ok(from_cache);
        }
        if !model_has_named_tests(model) {
            return Ok(from_cache);
        }
    }
    if let Some(nodeids) = lookup_python_file_nodeids(repo_root, &model.path) {
        return Ok(nodeids);
    }
    collect_python_nodeids_for_targets(
        repo_root,
        Some(std::slice::from_ref(&model.path)),
        pytest_args,
    )
}

pub(super) fn hydrate_python_models(
    repo_root: &Path,
    models: &mut BTreeMap<PathBuf, SourceModel>,
    pytest_args: &[String],
    cached_selectors: Option<&[String]>,
) -> Result<(), String> {
    let mut misses: Vec<PathBuf> = Vec::new();
    for (abs, model) in models.iter() {
        if model.language != kiss::Language::Python {
            continue;
        }
        if model.direct_tests.is_empty() && !is_python_test_module_path(abs) {
            continue;
        }
        let rel = repo_relative(repo_root, abs).unwrap_or_else(|| abs.display().to_string());
        if nodeids_already_available(repo_root, model, &rel, cached_selectors) {
            continue;
        }
        misses.push(abs.clone());
    }
    if misses.is_empty() {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let nodeids =
        collect_python_nodeids_for_targets(repo_root, Some(misses.as_slice()), pytest_args)?;
    crate::test_runner::emit_stage_time("python_target_batch_collect", started.elapsed());
    let mut updates: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for abs in &misses {
        let rel = repo_relative(repo_root, abs).unwrap_or_else(|| abs.display().to_string());
        let for_file = python_selectors_for_rel_path(&nodeids, &rel);
        updates.push((abs.clone(), for_file));
    }
    let _ = store_python_file_nodeids(repo_root, &updates);
    Ok(())
}

fn model_has_named_tests(model: &SourceModel) -> bool {
    model.direct_tests.iter().any(|test| !test.name.is_empty())
}

fn nodeids_already_available(
    repo_root: &Path,
    model: &SourceModel,
    rel: &str,
    cached_selectors: Option<&[String]>,
) -> bool {
    if let Some(cached) = cached_selectors {
        let from_cache = python_selectors_for_rel_path(cached, rel);
        if !from_cache.is_empty() {
            return true;
        }
        if !model_has_named_tests(model) {
            return true;
        }
    }
    lookup_python_file_nodeids(repo_root, &model.path).is_some()
}
