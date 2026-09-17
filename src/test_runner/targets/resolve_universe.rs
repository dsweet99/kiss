use std::path::Path;

use kiss::Language;

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
