use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kiss::Language;

use super::super::model::SourceModel;
use super::TargetSelectionQuery;
use super::resolve_insert::insert_direct;
use crate::test_runner::workspace_selector_cache::{
    load_cached_python_workspace_selectors, load_cached_rust_workspace_selectors,
    store_python_workspace_selectors, store_rust_workspace_selectors,
};

pub(super) fn flush_unresolved_universes(
    query: &mut TargetSelectionQuery,
    repo_root: &Path,
    ignore: &[String],
    pytest_args: &[String],
) -> Result<(), String> {
    if query.unresolved_python_test_module {
        insert_universe_if_unresolved(
            query,
            Language::Python,
            repo_root,
            ignore,
            pytest_args,
            0,
            usize::MAX,
        )?;
    }
    if query.unresolved_rust_test_module {
        insert_universe_if_unresolved(
            query,
            Language::Rust,
            repo_root,
            ignore,
            pytest_args,
            usize::MAX,
            0,
        )?;
    }
    Ok(())
}

fn insert_universe_if_unresolved(
    query: &mut TargetSelectionQuery,
    language: Language,
    repo_root: &Path,
    ignore: &[String],
    pytest_args: &[String],
    before_py: usize,
    before_rs: usize,
) -> Result<(), String> {
    match language {
        Language::Python if query.direct_python.len() == before_py => {
            let selectors =
                match load_cached_python_workspace_selectors(repo_root, ignore, pytest_args) {
                    Some(selectors) => selectors,
                    None => {
                        let selectors =
                            crate::test_runner::runners::enumerate_workspace_python_selectors(
                                repo_root,
                                ignore,
                                pytest_args,
                            )?;
                        store_python_workspace_selectors(
                            repo_root,
                            ignore,
                            &selectors,
                            pytest_args,
                        );
                        selectors
                    }
                };
            for selector in selectors {
                insert_direct(query, Language::Python, selector);
            }
        }
        Language::Rust if query.direct_rust.len() == before_rs => {
            let selectors = match load_cached_rust_workspace_selectors(repo_root, ignore) {
                Some(selectors) => selectors,
                None => {
                    let selectors =
                        crate::test_runner::runners::enumerate_workspace_rust_selectors(
                            repo_root, ignore,
                        )?;
                    store_rust_workspace_selectors(repo_root, ignore, &selectors);
                    selectors
                }
            };
            for selector in selectors {
                insert_direct(query, Language::Rust, selector);
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn qualify_rust_model(
    repo_root: &Path,
    model: &mut SourceModel,
    universe: &mut Option<BTreeSet<String>>,
) {
    if model.language != Language::Rust || model.direct_tests.is_empty() {
        return;
    }
    let universe = universe.get_or_insert_with(|| {
        crate::test_runner::runners::current_rust_selector_universe(repo_root)
    });
    let parsed: Vec<String> = model
        .direct_tests
        .iter()
        .map(|test| test.selector.clone())
        .collect();
    let mapped = crate::test_runner::runners::universe_rust_selectors_for_file(
        repo_root,
        &model.path,
        parsed.clone(),
        universe,
    );
    let renames: BTreeMap<String, String> = parsed
        .into_iter()
        .zip(mapped)
        .filter(|(from, to)| from != to)
        .collect();
    if renames.is_empty() {
        return;
    }
    for test in &mut model.direct_tests {
        if let Some(to) = renames.get(&test.selector) {
            test.selector = to.clone();
        }
    }
    for def in &mut model.definitions {
        if let Some(selector) = def.test_selector.as_mut()
            && let Some(to) = renames.get(selector)
        {
            *selector = to.clone();
        }
    }
}
