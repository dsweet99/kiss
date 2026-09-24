use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::resolved::ReverseRecord;

pub(crate) fn historical_covering_selectors(
    repo_root: &Path,
    historical_paths: &[String],
) -> Vec<String> {
    let abs: Vec<PathBuf> = historical_paths
        .iter()
        .map(|path| repo_root.join(path))
        .collect();
    let mut selectors = BTreeSet::new();
    if let Some(index) =
        crate::test_runner::python_coverage_index::load_current_python_coverage_index(repo_root)
    {
        for path in historical_paths {
            if let Some(found) = index.get(path) {
                selectors.extend(found.iter().cloned());
            }
        }
    }
    if let Some(python) =
        crate::test_runner::python_coverage_index::select_python_source_selectors_from_index(
            repo_root, &abs,
        )
    {
        selectors.extend(python);
    }
    if let Some(pop) = crate::test_runner::rust_coverage_index::load_current_rust_population_state(
        repo_root,
        None,
        &[],
    ) {
        for path in historical_paths {
            if let Some(found) = pop.line_index.get(path) {
                selectors.extend(found.iter().cloned());
            }
        }
        if let Some(rust) = crate::test_runner::rust_coverage_index::selectors_for_source_paths(
            repo_root,
            &abs,
            &pop.line_index,
        ) {
            selectors.extend(rust);
        }
    }
    selectors.into_iter().collect()
}

pub(crate) fn reverse_records(repo_root: &Path, paths: &[String]) -> Vec<ReverseRecord> {
    let mut records: Vec<ReverseRecord> = paths
        .iter()
        .filter(|path| index_has_path(repo_root, path))
        .map(|path| ReverseRecord {
            path: path.clone(),
            selectors: historical_covering_selectors(repo_root, std::slice::from_ref(path)),
        })
        .collect();
    records.sort_by(|left, right| left.path.cmp(&right.path));
    records
}

fn index_has_path(repo_root: &Path, path: &str) -> bool {
    let python =
        crate::test_runner::python_coverage_index::load_current_python_coverage_index(repo_root);
    let rust = crate::test_runner::rust_coverage_index::load_current_rust_population_state(
        repo_root,
        None,
        &[],
    );
    python
        .as_ref()
        .is_some_and(|index| index.contains_key(path))
        || rust
            .as_ref()
            .is_some_and(|pop| pop.line_index.contains_key(path))
}
