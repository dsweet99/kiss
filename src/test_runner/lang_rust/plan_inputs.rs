use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::runners::{
    ChangedTestSelectors, changed_lines_for_sources, changed_test_selectors_by_language,
    split_source_paths,
};

pub(crate) struct PreparedRustInputs {
    pub(crate) py_source_paths: Vec<PathBuf>,
    pub(crate) rust_source_paths: Vec<PathBuf>,
    pub(crate) python_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) changed_tests: ChangedTestSelectors,
}

/// Splits changed paths by language and finds the tests the changed test files define.
pub(crate) fn prepare_rust_inputs(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    ignore: &[String],
) -> Result<PreparedRustInputs, String> {
    let (py_source_paths, rust_source_paths) = split_source_paths(source_paths);
    let changed_tests = changed_test_selectors_by_language(repo_root, test_paths, ignore)?;
    Ok(PreparedRustInputs {
        python_changed_lines: changed_lines_for_sources(changed_lines, &py_source_paths),
        py_source_paths,
        rust_source_paths,
        changed_tests,
    })
}
