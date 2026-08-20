use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::enumerate_tests_in_changed_files;
#[path = "decision_prior.rs"]
mod decision_prior;
#[path = "decision_rust_paths.rs"]
mod decision_rust_paths;
use crate::test_runner::coverage_decision::{
    ChangedSource, CoverageDecisionEngine, LanguagePlanner, SelectionBasis, TestSelector,
};

#[cfg(test)]
use super::python_backer;
use super::python_backer::python_population_backer;
use super::rust_backer::{RustBackerInput, rust_llvm_cov_backer};
#[cfg(test)]
use super::rust_backer::{RustModule, select_fresh_rust_source_selectors};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorPlan {
    pub(crate) selectors: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub(crate) population_required: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    pub(crate) source_paths: crate::test_runner::language_keyed::LanguageKeyed<Vec<PathBuf>>,
    pub(crate) vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub(crate) snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub(crate) snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    pub(crate) changed_lines:
        crate::test_runner::language_keyed::LanguageKeyed<BTreeMap<PathBuf, BTreeSet<u32>>>,
    pub(crate) prior_failure_selectors:
        crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub(crate) coverage_decision_engine_used: bool,
    pub(crate) selection_basis: crate::test_runner::language_keyed::LanguageKeyed<SelectionBasis>,
}

#[derive(Clone, Copy)]
pub(crate) struct CombinedSelectorInput<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) source_paths: &'a [PathBuf],
    pub(crate) test_paths: &'a [PathBuf],
    pub(crate) changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) test_args: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub(crate) lang_filter: Option<kiss::Language>,
    pub(crate) ignore: &'a [String],
    pub(crate) extra_direct_python: &'a [String],
    pub(crate) extra_direct_rust: &'a [String],
    pub(crate) include_prior_failures: bool,
}

#[cfg(test)]
pub(crate) fn combined_selectors(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<SelectorPlan, String> {
    combined_selectors_with_direct(CombinedSelectorInput {
        repo_root,
        source_paths,
        test_paths,
        changed_lines: rust_changed_lines,
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: rust_test_args,
            rust: rust_test_args,
        },
        lang_filter,
        ignore,
        extra_direct_python: &[],
        extra_direct_rust: &[],
        include_prior_failures: true,
    })
}

pub(crate) fn combined_selectors_with_direct(
    input: CombinedSelectorInput<'_>,
) -> Result<SelectorPlan, String> {
    let covering_started = std::time::Instant::now();
    let plan = combined_selectors_with_direct_inner(input)?;
    crate::test_runner::emit_stage_time("covering_select", covering_started.elapsed());
    Ok(plan)
}

fn combined_selectors_with_direct_inner(
    input: CombinedSelectorInput<'_>,
) -> Result<SelectorPlan, String> {
    let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
    let mut mark = std::time::Instant::now();
    let mut lap = |label: &str| {
        if plan_trace {
            eprintln!("KISS_PLAN_TRACE {label}_ms={}", mark.elapsed().as_millis());
            mark = std::time::Instant::now();
        }
    };
    let mut prepared = decision_rust_paths::prepare_rust_inputs(
        input.repo_root,
        input.source_paths,
        input.test_paths,
        input.changed_lines,
        input.test_args.rust,
        input.lang_filter,
        input.ignore,
    )?;
    for selector in input.extra_direct_python {
        prepared
            .changed_tests
            .python
            .push(TestSelector::new(kiss::Language::Python, selector.clone()));
    }
    for selector in input.extra_direct_rust {
        prepared
            .changed_tests
            .rust
            .push(TestSelector::new(kiss::Language::Rust, selector.clone()));
    }
    lap("prepare_rust_inputs");
    let changed_sources =
        changed_sources_for_engine(&prepared.py_source_paths, &prepared.rust_source_paths);
    let engine_backers = engine_backers(EngineBackerInputs {
        repo_root: input.repo_root,
        py_source_paths: &prepared.py_source_paths,
        python_changed_lines: &prepared.python_changed_lines,
        rust_source_paths: &prepared.rust_source_paths,
        rust_changed_lines: &prepared.rust_changed_lines,
        test_args: input.test_args,
        lang_filter: input.lang_filter,
        ignore: input.ignore,
        changed_tests: &prepared.changed_tests,
        rust_resolved: prepared.rust_resolved.clone(),
        include_prior_failures: input.include_prior_failures,
    })?;
    lap("engine_backers");
    let plan = assemble_selector_plan(prepared, engine_backers, &changed_sources)?;
    lap("engine_plan");
    Ok(plan)
}

fn assemble_selector_plan(
    prepared: decision_rust_paths::PreparedRustInputs,
    engine_backers: EngineBackers,
    changed_sources: &[ChangedSource],
) -> Result<SelectorPlan, String> {
    let python_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Python);
    let rust_prior_failure_selectors =
        selectors_for_language(&engine_backers.prior_failures, kiss::Language::Rust);
    let mut selection_basis = engine_backers.selection_basis;
    let engine_plan = CoverageDecisionEngine::new(engine_backers.backers).plan(changed_sources)?;
    let (selected_py, selected_rs) = selectors_by_language(&engine_plan.selected);
    let (population_py, population_rs) = selectors_by_language(&engine_plan.population);
    let python_population_required = engine_plan
        .population_languages
        .contains(&kiss::Language::Python);
    let rust_population_required = engine_plan
        .population_languages
        .contains(&kiss::Language::Rust);
    if python_population_required {
        selection_basis.python = SelectionBasis::Population;
    }
    if rust_population_required {
        selection_basis.rust = SelectionBasis::Population;
    }
    let py_selectors = if python_population_required {
        population_py
    } else {
        selected_py
    };
    let rust_selectors = if rust_population_required {
        population_rs
    } else {
        selected_rs
    };
    Ok(SelectorPlan {
        selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: py_selectors,
            rust: rust_selectors,
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: python_population_required,
            rust: rust_population_required,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: prepared.py_source_paths,
            rust: prepared.rust_source_paths,
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: prepared.rust_vcs_source_paths,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: prepared.rust_snapshot_delta_modified,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: prepared.rust_snapshot_delta_structural,
        },
        changed_lines: crate::test_runner::language_keyed::LanguageKeyed {
            python: prepared.python_changed_lines,
            rust: prepared.rust_changed_lines,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: python_prior_failure_selectors,
            rust: rust_prior_failure_selectors,
        },
        coverage_decision_engine_used: true,
        selection_basis,
    })
}

struct EngineBackerInputs<'a> {
    repo_root: &'a Path,
    py_source_paths: &'a [PathBuf],
    python_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_source_paths: &'a [PathBuf],
    rust_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    lang_filter: Option<kiss::Language>,
    ignore: &'a [String],
    changed_tests: &'a ChangedTestSelectors,
    rust_resolved: Option<crate::test_runner::rust_coverage_index::ResolvedRustPopulation>,
    include_prior_failures: bool,
}

struct EngineBackers {
    backers: Vec<Box<dyn LanguagePlanner>>,
    prior_failures: Vec<TestSelector>,
    selection_basis: crate::test_runner::language_keyed::LanguageKeyed<SelectionBasis>,
}

fn engine_backers(input: EngineBackerInputs<'_>) -> Result<EngineBackers, String> {
    let mut backers = Vec::new();
    let python_prior_failures =
        if !input.include_prior_failures || input.lang_filter == Some(kiss::Language::Rust) {
            Vec::new()
        } else {
            prior_failures_for_language(
                input.repo_root,
                kiss::Language::Python,
                input.test_args.python,
            )?
        };
    let rust_prior_failures = if !input.include_prior_failures
        || input.lang_filter == Some(kiss::Language::Python)
    {
        Vec::new()
    } else {
        prior_failures_for_language(input.repo_root, kiss::Language::Rust, input.test_args.rust)?
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
            input.test_args.python,
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
                .is_some_and(|resolved| resolved.basis() == SelectionBasis::Population))
    {
        backers.push(rust_llvm_cov_backer(RustBackerInput {
            repo_root: input.repo_root,
            rust_source_paths: input.rust_source_paths,
            rust_changed_lines: input.rust_changed_lines,
            rust_test_args: input.test_args.rust,
            ignore: input.ignore,
            changed_tests: &input.changed_tests.rust,
            prior_failures: &rust_prior_failures,
            resolved: input.rust_resolved,
        }));
    }
    let mut selection_basis = crate::test_runner::language_keyed::LanguageKeyed::default();
    for backer in &backers {
        *selection_basis.get_mut(backer.language()) = backer.selection_basis();
    }
    let mut prior_failures = python_prior_failures;
    prior_failures.extend(rust_prior_failures);
    Ok(EngineBackers {
        backers,
        prior_failures,
        selection_basis,
    })
}

pub(crate) use decision_prior::prior_failures_for_language;

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
