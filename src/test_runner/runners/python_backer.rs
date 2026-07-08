use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::super::enumerate_workspace_python_selectors;
use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageBacker, CoverageFreshness, PopulationPlan, SelectionDecision, TestSelector,
};
use crate::test_runner::python_coverage_index::{
    python_population_manifest_is_current_for_args, select_python_source_selectors_from_index,
    select_python_source_selectors_hybrid,
};

pub(super) fn python_population_backer(
    repo_root: &Path,
    py_source_paths: &[PathBuf],
    python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
    ignore: &[String],
    changed_tests: &[TestSelector],
    prior_failures: &[TestSelector],
) -> CoverageBacker {
    let repo_root = repo_root.to_path_buf();
    let py_source_paths = py_source_paths.to_vec();
    let python_changed_lines = python_changed_lines.clone();
    let test_args = test_args.to_vec();
    let ignore = ignore.to_vec();
    let changed_tests = changed_tests.to_vec();
    let prior_failures = prior_failures.to_vec();
    CoverageBacker::new(
        kiss::Language::Python,
        Box::new({
            let repo_root = repo_root.clone();
            let ignore = ignore.clone();
            move || {
                Ok(enumerate_workspace_python_selectors(&repo_root, &ignore)?
                    .into_iter()
                    .map(|id| TestSelector::new(kiss::Language::Python, id))
                    .collect())
            }
        }),
        Box::new(move |_diff: &ChangedDiff| changed_tests.clone()),
        Box::new(move || prior_failures.clone()),
        Box::new({
            let py_source_paths = py_source_paths.clone();
            let repo_root = repo_root.clone();
            let test_args = test_args.clone();
            move |universe| {
                if py_source_paths.is_empty() {
                    return Ok(CoverageFreshness::Fresh);
                }
                let universe_ids = universe
                    .iter()
                    .map(|selector| selector.id.clone())
                    .collect::<Vec<_>>();
                if python_population_manifest_is_current_for_args(
                    &repo_root,
                    &universe_ids,
                    &test_args,
                ) {
                    Ok(CoverageFreshness::Fresh)
                } else {
                    Ok(CoverageFreshness::Stale)
                }
            }
        }),
        Box::new(|universe| PopulationPlan {
            selectors: universe.to_vec(),
        }),
        Box::new(move |_changed_sources| {
            let Some(selector_ids) = select_fresh_python_source_selectors(
                &repo_root,
                &py_source_paths,
                &python_changed_lines,
            ) else {
                return Ok(SelectionDecision {
                    selectors: Vec::new(),
                    complete: false,
                });
            };
            Ok(SelectionDecision {
                selectors: selector_ids
                    .into_iter()
                    .map(|id| TestSelector::new(kiss::Language::Python, id))
                    .collect(),
                complete: true,
            })
        }),
    )
}

fn select_fresh_python_source_selectors(
    repo_root: &Path,
    py_source_paths: &[PathBuf],
    python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if !python_changed_lines.is_empty()
        && let Some(line_selectors) =
            select_python_source_selectors_hybrid(repo_root, py_source_paths, python_changed_lines)
    {
        return Some(line_selectors);
    }
    select_python_source_selectors_from_index(repo_root, py_source_paths)
}
