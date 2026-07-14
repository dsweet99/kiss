use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::enumerate_tests_in_changed_files;
#[path = "decision_rust_paths.rs"]
mod decision_rust_paths;
use crate::test_runner::coverage_decision::{
    ChangedSource, CoverageDecisionEngine, LanguagePlanner, RustSelectionBasis, TestSelector,
};
use crate::test_runner::last_status::{
    has_language_records, prior_failures, python_last_status_identity, rust_last_status_identity,
};

#[cfg(test)]
use super::python_backer;
use super::python_backer::python_population_backer;
use super::rust_backer::{RustBackerInput, rust_llvm_cov_backer};
#[cfg(test)]
use super::rust_backer::{RustModule, select_fresh_rust_source_selectors};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorPlan {
    pub(crate) py_selectors: Vec<String>,
    pub(crate) rust_selectors: Vec<String>,
    pub(crate) python_population_required: bool,
    pub(crate) rust_population_required: bool,
    pub(crate) rust_source_paths: Vec<PathBuf>,
    pub(crate) rust_vcs_source_paths: usize,
    pub(crate) rust_snapshot_delta_modified: usize,
    pub(crate) rust_snapshot_delta_structural: bool,
    pub(crate) python_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) python_prior_failure_selectors: Vec<String>,
    pub(crate) rust_prior_failure_selectors: Vec<String>,
    pub(crate) coverage_decision_engine_used: bool,
    pub(crate) rust_selection_basis: RustSelectionBasis,
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
    let prepared = decision_rust_paths::prepare_rust_inputs(
        repo_root,
        source_paths,
        test_paths,
        rust_changed_lines,
        rust_test_args,
        lang_filter,
        ignore,
    )?;
    let changed_sources =
        changed_sources_for_engine(&prepared.py_source_paths, &prepared.rust_source_paths);
    let engine_backers = engine_backers(EngineBackerInputs {
        repo_root,
        py_source_paths: &prepared.py_source_paths,
        python_changed_lines: &prepared.python_changed_lines,
        rust_source_paths: &prepared.rust_source_paths,
        rust_changed_lines: &prepared.rust_changed_lines,
        rust_test_args,
        lang_filter,
        ignore,
        changed_tests: &prepared.changed_tests,
        rust_resolved: prepared.rust_resolved,
    })?;
    let python_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Python);
    let rust_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Rust);
    let pre_rust_selection_basis = rust_selection_basis_from_backers(&engine_backers.backers);
    let engine_plan = CoverageDecisionEngine::new(engine_backers.backers).plan(&changed_sources)?;
    let (selected_py, selected_rs) = selectors_by_language(&engine_plan.selected);
    let (population_py, population_rs) = selectors_by_language(&engine_plan.population);
    let python_population_required = engine_plan
        .population_languages
        .contains(&kiss::Language::Python);
    let rust_population_required = engine_plan
        .population_languages
        .contains(&kiss::Language::Rust);
    let rust_selection_basis = if rust_population_required {
        RustSelectionBasis::Population
    } else {
        pre_rust_selection_basis
    };
    Ok(SelectorPlan {
        py_selectors: if python_population_required {
            population_py
        } else {
            selected_py
        },
        rust_selectors: if rust_population_required {
            population_rs
        } else {
            selected_rs
        },
        python_population_required,
        rust_population_required,
        rust_source_paths: prepared.rust_source_paths,
        rust_vcs_source_paths: prepared.rust_vcs_source_paths,
        rust_snapshot_delta_modified: prepared.rust_snapshot_delta_modified,
        rust_snapshot_delta_structural: prepared.rust_snapshot_delta_structural,
        python_changed_lines: prepared.python_changed_lines,
        rust_changed_lines: prepared.rust_changed_lines,
        python_prior_failure_selectors,
        rust_prior_failure_selectors,
        coverage_decision_engine_used: true,
        rust_selection_basis,
    })
}

fn rust_selection_basis_from_backers(backers: &[Box<dyn LanguagePlanner>]) -> RustSelectionBasis {
    for backer in backers {
        if let Some(basis) = backer.rust_selection_basis() {
            return basis;
        }
    }
    RustSelectionBasis::Current
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
    rust_resolved: Option<crate::test_runner::rust_coverage_index::ResolvedRustPopulation>,
}

struct EngineBackers {
    backers: Vec<Box<dyn LanguagePlanner>>,
    prior_failures: Vec<TestSelector>,
}

impl EngineBackers {
    fn new(backers: Vec<Box<dyn LanguagePlanner>>, prior_failures: Vec<TestSelector>) -> Self {
        EngineBackers {
            backers,
            prior_failures,
        }
    }
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
            || !rust_prior_failures.is_empty()
            || input
                .rust_resolved
                .as_ref()
                .is_some_and(|resolved| resolved.basis == RustSelectionBasis::Population))
    {
        backers.push(rust_llvm_cov_backer(RustBackerInput {
            repo_root: input.repo_root,
            rust_source_paths: input.rust_source_paths,
            rust_changed_lines: input.rust_changed_lines,
            rust_test_args: input.rust_test_args,
            ignore: input.ignore,
            changed_tests: &input.changed_tests.rust,
            prior_failures: &rust_prior_failures,
            resolved: input.rust_resolved,
        }));
    }
    let mut prior_failures = python_prior_failures;
    prior_failures.extend(rust_prior_failures);
    Ok(EngineBackers::new(backers, prior_failures))
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
            let cargo_version = super::command_stdout(&cargo, &["--version"], repo_root)?;
            let llvm_cov_version =
                super::command_stdout(&cargo, &["llvm-cov", "--version"], repo_root)?;
            let cargo_nextest_version =
                super::command_stdout(&cargo, &["nextest", "--version"], repo_root)?;
            let rustc_version = super::command_stdout(&rustc, &["-Vv"], repo_root)?;
            let runner_map_fingerprint =
                crate::test_runner::rust_coverage_index::current_rust_runner_map_fingerprint(
                    repo_root, test_args,
                )?;
            rust_last_status_identity(
                &cargo_version,
                &llvm_cov_version,
                &rustc_version,
                &cargo_nextest_version,
                test_args,
                &runner_map_fingerprint,
            )
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
    repo_root: &Path,
    test_paths: &[PathBuf],
) -> Result<ChangedTestSelectors, String> {
    let enumerated = enumerate_tests_in_changed_files(repo_root, test_paths)?;
    let mut changed = ChangedTestSelectors::default();
    for nodeid in enumerated.python_nodeids {
        changed
            .python
            .push(TestSelector::new(kiss::Language::Python, nodeid));
    }
    for (path, id) in enumerated.rust_tests {
        if kiss::Language::is_rust_path(&path) {
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
        .partition(|path| !super::is_rust_planning_source_path(path))
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

#[cfg(test)]
#[path = "decision_test.rs"]
mod tests;
