use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{enumerate_tests_in_changed_files, enumerate_workspace_rust_selectors, py_selector};
use crate::test_runner::coverage_decision::{
    ChangedDiff, ChangedSource, CoverageBacker, CoverageDecisionEngine, CoverageFreshness,
    PopulationPlan, SelectionDecision, TestSelector,
};
use crate::test_runner::last_status::{
    has_language_records, prior_failures, python_last_status_identity, rust_last_status_identity,
};
use crate::test_runner::rust_coverage_index::{
    rust_population_manifest_is_current_for_args, select_rust_source_selectors_from_index,
    select_rust_source_selectors_hybrid,
};

#[path = "python_backer.rs"]
mod python_backer;
use python_backer::python_population_backer;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorPlan {
    pub(crate) py_selectors: Vec<String>,
    pub(crate) rust_selectors: Vec<String>,
    pub(crate) python_population_required: bool,
    pub(crate) python_population_selectors: Vec<String>,
    pub(crate) rust_source_paths: Vec<PathBuf>,
    pub(crate) python_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) rust_source_population_paths: Vec<PathBuf>,
    pub(crate) python_prior_failure_selectors: Vec<String>,
    pub(crate) rust_prior_failure_selectors: Vec<String>,
    pub(crate) coverage_decision_engine_used: bool,
}

pub(crate) fn combined_selectors(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<SelectorPlan, String> {
    let (py_source_paths, rust_source_paths) = split_source_paths(source_paths);
    let python_changed_lines = changed_lines_for_sources(rust_changed_lines, &py_source_paths);
    let rust_changed_lines = rust_changed_lines_for_sources(rust_changed_lines, &rust_source_paths);
    let changed_tests = changed_test_selectors_by_language(test_paths)?;
    let changed_sources = changed_sources_for_engine(&py_source_paths, &rust_source_paths);
    let engine_backers = engine_backers(EngineBackerInputs {
        repo_root,
        py_source_paths: &py_source_paths,
        python_changed_lines: &python_changed_lines,
        rust_source_paths: &rust_source_paths,
        rust_changed_lines: &rust_changed_lines,
        rust_test_args,
        lang_filter,
        ignore,
        changed_tests: &changed_tests,
    })?;
    let python_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Python);
    let rust_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Rust);
    let engine_plan = CoverageDecisionEngine::new(engine_backers.backers).plan(&changed_sources)?;
    let (py_sel, rs_sel) = selectors_by_language(&engine_plan.selected);
    let python_population_selectors =
        selectors_for_language(&engine_plan.population, kiss::Language::Python);
    let python_population_required = engine_plan
        .population_languages
        .contains(&kiss::Language::Python);
    let rust_source_population_paths = if engine_plan
        .population_languages
        .contains(&kiss::Language::Rust)
    {
        rust_source_paths.clone()
    } else {
        Vec::new()
    };
    Ok(SelectorPlan {
        py_selectors: py_sel,
        rust_selectors: rs_sel,
        python_population_required,
        python_population_selectors,
        rust_source_paths,
        python_changed_lines: python_changed_lines.clone(),
        rust_changed_lines: rust_changed_lines.clone(),
        rust_source_population_paths,
        python_prior_failure_selectors,
        rust_prior_failure_selectors,
        coverage_decision_engine_used: true,
    })
}

struct EngineBackerInputs<'a> {
    repo_root: &'a Path,
    py_source_paths: &'a [PathBuf],
    python_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_source_paths: &'a [PathBuf],
    rust_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &'a [String],
    lang_filter: Option<kiss::Language>,
    ignore: &'a [String],
    changed_tests: &'a ChangedTestSelectors,
}

struct EngineBackers {
    backers: Vec<CoverageBacker>,
    prior_failures: Vec<TestSelector>,
}

fn engine_backers(input: EngineBackerInputs<'_>) -> Result<EngineBackers, String> {
    let mut backers = Vec::new();
    let python_prior_failures = if input.lang_filter == Some(kiss::Language::Rust) {
        Vec::new()
    } else {
        prior_failures_for_language(
            input.repo_root,
            kiss::Language::Python,
            input.rust_test_args,
        )?
    };
    let rust_prior_failures = if input.lang_filter == Some(kiss::Language::Python) {
        Vec::new()
    } else {
        prior_failures_for_language(input.repo_root, kiss::Language::Rust, input.rust_test_args)?
    };
    if input.lang_filter != Some(kiss::Language::Rust)
        && (!input.py_source_paths.is_empty()
            || !input.changed_tests.python.is_empty()
            || !python_prior_failures.is_empty())
    {
        backers.push(python_population_backer(
            input.repo_root,
            input.py_source_paths,
            input.python_changed_lines,
            input.rust_test_args,
            input.ignore,
            &input.changed_tests.python,
            &python_prior_failures,
        ));
    }
    if input.lang_filter != Some(kiss::Language::Python)
        && (!input.rust_source_paths.is_empty()
            || !input.changed_tests.rust.is_empty()
            || !rust_prior_failures.is_empty())
    {
        backers.push(rust_llvm_cov_backer(
            input.repo_root,
            input.rust_source_paths,
            input.rust_changed_lines,
            input.rust_test_args,
            input.ignore,
            &input.changed_tests.rust,
            &rust_prior_failures,
        ));
    }
    let mut prior_failures = python_prior_failures;
    prior_failures.extend(rust_prior_failures);
    Ok(EngineBackers {
        backers,
        prior_failures,
    })
}

fn prior_failures_for_language(
    repo_root: &Path,
    language: kiss::Language,
    test_args: &[String],
) -> Result<Vec<TestSelector>, String> {
    if !has_language_records(repo_root, language)? {
        return Ok(Vec::new());
    }
    let identity = match language {
        kiss::Language::Python => {
            let python = PathBuf::from("python");
            let python_version = super::command_stdout(
                &python,
                &[
                    "-c",
                    "import sys; print('.'.join(map(str, sys.version_info[:3])))",
                ],
                repo_root,
            )?;
            let pytest_version = super::command_stdout(
                &python,
                &["-c", "import pytest; print(pytest.__version__)"],
                repo_root,
            )?;
            python_last_status_identity(&python_version, &pytest_version, test_args)
        }
        kiss::Language::Rust => {
            let cargo = PathBuf::from("cargo");
            let rustc = PathBuf::from("rustc");
            let llvm_cov_version =
                super::command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?;
            let rustc_version = super::command_stdout(&rustc, &["-Vv"], repo_root)?;
            rust_last_status_identity(&llvm_cov_version, &rustc_version, test_args)
        }
    };
    Ok(prior_failures(repo_root, language, &identity)?
        .into_iter()
        .map(|id| TestSelector::new(language, id))
        .collect())
}

fn selectors_for_language(selectors: &[TestSelector], language: kiss::Language) -> Vec<String> {
    selectors
        .iter()
        .filter(|selector| selector.language == language)
        .map(|selector| selector.id.clone())
        .collect()
}

fn rust_llvm_cov_backer(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
    ignore: &[String],
    changed_tests: &[TestSelector],
    prior_failures: &[TestSelector],
) -> CoverageBacker {
    let repo_root = repo_root.to_path_buf();
    let rust_source_paths = rust_source_paths.to_vec();
    let rust_changed_lines = rust_changed_lines.clone();
    let rust_test_args = rust_test_args.to_vec();
    let ignore = ignore.to_vec();
    let changed_tests = changed_tests.to_vec();
    let prior_failures = prior_failures.to_vec();
    CoverageBacker::new(
        kiss::Language::Rust,
        Box::new({
            let repo_root = repo_root.clone();
            let ignore = ignore.clone();
            move || {
                Ok(enumerate_workspace_rust_selectors(&repo_root, &ignore)?
                    .into_iter()
                    .map(|id| TestSelector::new(kiss::Language::Rust, id))
                    .collect())
            }
        }),
        Box::new(move |_diff: &ChangedDiff| changed_tests.clone()),
        Box::new(move || prior_failures.clone()),
        Box::new({
            let repo_root = repo_root.clone();
            let rust_source_paths = rust_source_paths.clone();
            let rust_test_args = rust_test_args.clone();
            move |universe| {
                if rust_source_paths.is_empty() {
                    return Ok(CoverageFreshness::Fresh);
                }
                let universe_ids = universe
                    .iter()
                    .map(|selector| selector.id.clone())
                    .collect::<Vec<_>>();
                if rust_population_manifest_is_current_for_args(
                    &repo_root,
                    &universe_ids,
                    &rust_test_args,
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
            let Some(selector_ids) = select_fresh_rust_source_selectors(
                &repo_root,
                &rust_source_paths,
                &rust_changed_lines,
            ) else {
                return Ok(SelectionDecision {
                    selectors: Vec::new(),
                    complete: false,
                });
            };
            let selectors = selector_ids
                .into_iter()
                .map(|id| TestSelector::new(kiss::Language::Rust, id))
                .collect();
            Ok(SelectionDecision {
                selectors,
                complete: true,
            })
        }),
    )
}

fn changed_sources_for_engine(
    py_source_paths: &[PathBuf],
    rust_source_paths: &[PathBuf],
) -> Vec<ChangedSource> {
    py_source_paths
        .iter()
        .map(|path| ChangedSource::new(kiss::Language::Python, path.to_string_lossy()))
        .chain(
            rust_source_paths
                .iter()
                .map(|path| ChangedSource::new(kiss::Language::Rust, path.to_string_lossy())),
        )
        .collect()
}

#[derive(Default)]
struct ChangedTestSelectors {
    python: Vec<TestSelector>,
    rust: Vec<TestSelector>,
}

fn changed_test_selectors_by_language(
    test_paths: &[PathBuf],
) -> Result<ChangedTestSelectors, String> {
    let mut changed = ChangedTestSelectors::default();
    for (path, id) in enumerate_tests_in_changed_files(test_paths)? {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("py"))
        {
            changed.python.push(TestSelector::new(
                kiss::Language::Python,
                py_selector(&path, &id),
            ));
        } else if kiss::Language::is_rust_path(&path) {
            changed
                .rust
                .push(TestSelector::new(kiss::Language::Rust, id));
        }
    }
    Ok(changed)
}

fn selectors_by_language(selectors: &[TestSelector]) -> (Vec<String>, Vec<String>) {
    let mut py_sel = BTreeSet::new();
    let mut rs_sel = BTreeSet::new();
    for selector in selectors {
        match selector.language {
            kiss::Language::Python => {
                py_sel.insert(selector.id.clone());
            }
            kiss::Language::Rust => {
                rs_sel.insert(selector.id.clone());
            }
        }
    }
    (py_sel.into_iter().collect(), rs_sel.into_iter().collect())
}

fn split_source_paths(source_paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    source_paths
        .iter()
        .cloned()
        .partition(|path| !kiss::Language::is_rust_path(path))
}

fn rust_changed_lines_for_sources(
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_source_paths: &[PathBuf],
) -> BTreeMap<PathBuf, BTreeSet<u32>> {
    changed_lines_for_sources(rust_changed_lines, rust_source_paths)
}

fn changed_lines_for_sources(
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    source_paths: &[PathBuf],
) -> BTreeMap<PathBuf, BTreeSet<u32>> {
    changed_lines
        .iter()
        .filter(|(path, _lines)| source_paths.contains(path))
        .map(|(path, lines)| (path.clone(), lines.clone()))
        .collect()
}

fn select_fresh_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if !rust_changed_lines.is_empty()
        && let Some(line_selectors) =
            select_rust_source_selectors_hybrid(repo_root, rust_source_paths, rust_changed_lines)
    {
        return Some(line_selectors);
    }
    select_rust_source_selectors_from_index(repo_root, rust_source_paths)
}

#[cfg(test)]
#[path = "decision_test.rs"]
mod tests;
