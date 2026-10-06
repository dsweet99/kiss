use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kiss::code_roles::{RustTestBinaryModule, workspace_rust_test_modules};
use kiss::rust_include::canonical_path;

type ModulesByFile = HashMap<PathBuf, Vec<RustTestBinaryModule>>;

/// The crate-relative test path of a Rust selector: the part after `$` for a selector
/// qualified as `<nextest binary id>$<test path>` (binary id may be empty), else the selector.
pub(crate) fn rust_selector_test_path(selector: &str) -> &str {
    selector
        .split_once('$')
        .map_or(selector, |(_, test_path)| test_path)
}

fn qualified_selector(modules: &[RustTestBinaryModule], selector: &str) -> Option<String> {
    let first = modules.first()?;
    if modules
        .iter()
        .any(|module| module.module_path != first.module_path)
    {
        return None;
    }
    let test_path = if first.module_path.is_empty() {
        selector.to_string()
    } else {
        format!("{}::{selector}", first.module_path)
    };
    if modules.len() == 1 {
        Some(format!("{}${test_path}", first.binary_prefix))
    } else {
        Some(format!("${test_path}"))
    }
}

fn qualify_with(modules: &ModulesByFile, path: &Path, selector: &str) -> Option<String> {
    qualified_selector(modules.get(&canonical_path(path))?, selector)
}

/// Replaces every selector defined in more than one file with its qualified form, so
/// same-named tests in different files stay distinct tests.
pub(super) fn qualify_colliding_entries(
    repo_root: &Path,
    entries: Vec<(PathBuf, String)>,
) -> Vec<(PathBuf, String)> {
    let mut files_by_selector: BTreeMap<&str, BTreeSet<&Path>> = BTreeMap::new();
    for (path, selector) in &entries {
        files_by_selector
            .entry(selector.as_str())
            .or_default()
            .insert(path.as_path());
    }
    let colliding: BTreeSet<String> = files_by_selector
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(selector, _)| selector.to_string())
        .collect();
    if colliding.is_empty() {
        return entries;
    }
    let Ok(modules) = workspace_rust_test_modules(repo_root) else {
        return entries;
    };
    entries
        .into_iter()
        .map(|(path, selector)| {
            if !colliding.contains(&selector) {
                return (path, selector);
            }
            let qualified = qualify_with(&modules, &path, &selector).unwrap_or(selector);
            (path, qualified)
        })
        .collect()
}

/// Maps selectors parsed from one file to the selectors the workspace universe uses for them.
pub(crate) fn universe_rust_selectors_for_file(
    repo_root: &Path,
    path: &Path,
    selectors: Vec<String>,
    universe: &BTreeSet<String>,
) -> Vec<String> {
    if universe.is_empty() || selectors.iter().all(|selector| universe.contains(selector)) {
        return selectors;
    }
    let Ok(modules) = workspace_rust_test_modules(repo_root) else {
        return selectors;
    };
    selectors
        .into_iter()
        .map(|selector| {
            if universe.contains(&selector) {
                return selector;
            }
            match qualify_with(&modules, path, &selector) {
                Some(qualified) if universe.contains(&qualified) => qualified,
                _ => selector,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "rust_selector_qualify_test.rs"]
mod tests;
