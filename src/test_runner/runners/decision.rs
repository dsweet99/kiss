use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::enumerate_tests_in_changed_files;
#[path = "decision_prior.rs"]
mod decision_prior;
use crate::test_runner::lang_rust::plan_inputs as decision_rust_paths;
#[path = "importer_select.rs"]
mod importer_select;
use crate::test_runner::test_selection::{
    ChangedSource, LanguagePlanner, SelectionBasis, SelectionEngine, TestSelector,
};

use crate::test_runner::language_keyed::LanguageKeyed;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorPlan {
    pub(crate) selectors: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub(crate) population_required: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    pub(crate) source_paths: crate::test_runner::language_keyed::LanguageKeyed<Vec<PathBuf>>,
    pub(crate) vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub(crate) prior_failure_selectors:
        crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub(crate) selection_engine_used: bool,
    pub(crate) selection_basis: crate::test_runner::language_keyed::LanguageKeyed<SelectionBasis>,
}

#[derive(Clone, Copy)]
pub(crate) struct CombinedSelectorInput<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) source_paths: &'a [PathBuf],
    pub(crate) test_paths: &'a [PathBuf],
    pub(crate) test_args: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub(crate) lang_filter: Option<kiss::Language>,
    pub(crate) ignore: &'a [String],
    pub(crate) extra_direct: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub(crate) include_prior_failures: bool,
}

#[cfg(test)]
pub(crate) fn combined_selectors(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    rust_test_args: &[String],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<SelectorPlan, String> {
    combined_selectors_with_direct(CombinedSelectorInput {
        repo_root,
        source_paths,
        test_paths,
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: rust_test_args,
            rust: rust_test_args,
        },
        lang_filter,
        ignore,
        extra_direct: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        include_prior_failures: true,
    })
}

pub(crate) fn combined_selectors_with_direct(
    input: CombinedSelectorInput<'_>,
) -> Result<SelectorPlan, String> {
    let selecting_started = std::time::Instant::now();
    let plan = combined_selectors_with_direct_inner(input)?;
    crate::test_runner::emit_stage_time("selection", selecting_started.elapsed());
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
        input.ignore,
    )?;
    extend_tagged(&mut prepared.changed_tests, input.extra_direct.owned_vecs());
    lap("prepare_rust_inputs");
    let changed_sources =
        changed_sources_for_engine(&prepared.py_source_paths, &prepared.rust_source_paths);
    let engine_backers = engine_backers(EngineBackerInputs {
        repo_root: input.repo_root,
        py_source_paths: &prepared.py_source_paths,
        rust_source_paths: &prepared.rust_source_paths,
        test_args: input.test_args,
        lang_filter: input.lang_filter,
        ignore: input.ignore,
        changed_tests: &prepared.changed_tests,
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
    let prior_failure_selectors = LanguageKeyed::from_fn(|language| {
        selectors_for_language(&engine_backers.prior_failures, language)
    });
    let mut selection_basis = engine_backers.selection_basis;
    let rust_vcs_source_paths = prepared.rust_source_paths.len();
    let engine_plan = SelectionEngine::new(engine_backers.backers).plan(changed_sources)?;
    let mut selected = keyed_selectors(&engine_plan.selected);
    let mut population = keyed_selectors(&engine_plan.population);
    let population_required =
        LanguageKeyed::from_fn(|language| engine_plan.population_languages.contains(&language));
    let selectors = LanguageKeyed::from_fn(|language| {
        if *population_required.get(language) {
            *selection_basis.get_mut(language) = SelectionBasis::Population;
            std::mem::take(population.get_mut(language))
        } else {
            std::mem::take(selected.get_mut(language))
        }
    });
    Ok(SelectorPlan {
        selectors,
        population_required,
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: prepared.py_source_paths,
            rust: prepared.rust_source_paths,
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: rust_vcs_source_paths,
        },
        prior_failure_selectors,
        selection_engine_used: true,
        selection_basis,
    })
}

struct EngineBackerInputs<'a> {
    repo_root: &'a Path,
    py_source_paths: &'a [PathBuf],
    rust_source_paths: &'a [PathBuf],
    test_args: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    lang_filter: Option<kiss::Language>,
    ignore: &'a [String],
    changed_tests: &'a ChangedTestSelectors,
    include_prior_failures: bool,
}

struct EngineBackers {
    backers: Vec<Box<dyn LanguagePlanner>>,
    prior_failures: Vec<TestSelector>,
    selection_basis: crate::test_runner::language_keyed::LanguageKeyed<SelectionBasis>,
}

fn engine_backers(input: EngineBackerInputs<'_>) -> Result<EngineBackers, String> {
    let mut prior = LanguageKeyed::<Vec<TestSelector>>::default();
    for language in crate::test_runner::lang_registry::languages() {
        if input.include_prior_failures && language.allowed_by(input.lang_filter) {
            *prior.get_mut(language) = current_prior_failures(
                input.repo_root,
                language,
                input.test_args.get(language),
                input.ignore,
            )?;
        }
    }
    let source_paths = LanguageKeyed {
        python: input.py_source_paths,
        rust: input.rust_source_paths,
    };
    let mut backers = Vec::new();
    for language in crate::test_runner::lang_registry::languages() {
        let has_work = !source_paths.get(language).is_empty()
            || !input.changed_tests.get(language).is_empty()
            || !prior.get(language).is_empty();
        if !language.allowed_by(input.lang_filter) || !has_work {
            continue;
        }
        backers.push(crate::test_runner::lang_registry::planner_backer(
            language,
            crate::test_runner::lang_registry::PlannerBackerInput {
                repo_root: input.repo_root,
                source_paths: source_paths.get(language),
                test_args: input.test_args.get(language),
                ignore: input.ignore,
                changed_tests: input.changed_tests.get(language),
                prior_failures: prior.get(language),
            },
        ));
    }
    let mut selection_basis = LanguageKeyed::default();
    for backer in &backers {
        *selection_basis.get_mut(backer.language()) = backer.selection_basis();
    }
    let prior_failures = crate::test_runner::lang_registry::languages()
        .into_iter()
        .flat_map(|language| std::mem::take(prior.get_mut(language)))
        .collect();
    Ok(EngineBackers {
        backers,
        prior_failures,
        selection_basis,
    })
}

pub(crate) fn current_prior_failures(
    repo_root: &Path,
    language: kiss::Language,
    test_args: &[String],
    ignore: &[String],
) -> Result<Vec<TestSelector>, String> {
    let prior = prior_failures_for_language(repo_root, language);
    if prior.is_empty() {
        return Ok(prior);
    }
    let current: BTreeSet<String> = crate::test_runner::lang_registry::cached_workspace_selectors(
        repo_root, language, ignore, test_args,
    )?
    .into_iter()
    .collect();
    Ok(prior
        .into_iter()
        .filter(|selector| current.contains(&selector.id))
        .collect())
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
    let source_paths = LanguageKeyed {
        python: py_source_paths,
        rust: rust_source_paths,
    };
    crate::test_runner::lang_registry::languages()
        .into_iter()
        .flat_map(|language| {
            source_paths
                .get(language)
                .iter()
                .map(move |path| ChangedSource::new(language, path.to_string_lossy()))
        })
        .collect()
}

pub(crate) type ChangedTestSelectors = LanguageKeyed<Vec<TestSelector>>;

fn extend_tagged(to: &mut ChangedTestSelectors, ids: LanguageKeyed<Vec<String>>) {
    let mut ids = ids;
    for language in crate::test_runner::lang_registry::languages() {
        to.get_mut(language).extend(
            std::mem::take(ids.get_mut(language))
                .into_iter()
                .map(|id| TestSelector::new(language, id)),
        );
    }
}

fn changed_file_ids(tests: &super::ChangedFileTests) -> LanguageKeyed<Vec<String>> {
    LanguageKeyed {
        python: tests.python_nodeids.iter().cloned().collect(),
        rust: tests
            .rust_tests
            .iter()
            .filter(|(path, _)| kiss::Language::is_rust_path(path))
            .map(|(_, id)| id.clone())
            .collect(),
    }
}

pub(crate) fn changed_test_selectors_by_language(
    repo_root: &Path,
    test_paths: &[PathBuf],
    ignore: &[String],
) -> Result<ChangedTestSelectors, String> {
    let enumerated = enumerate_tests_in_changed_files(repo_root, test_paths)?;
    let mut changed = ChangedTestSelectors::default();
    extend_tagged(&mut changed, changed_file_ids(&enumerated));
    importer_select::expand_unresolved_test_helpers(
        repo_root,
        test_paths,
        ignore,
        &enumerated,
        &mut changed,
    )?;
    importer_select::append_importer_tests(repo_root, ignore, test_paths, &mut changed)?;
    Ok(changed)
}

fn keyed_selectors(selectors: &[TestSelector]) -> LanguageKeyed<Vec<String>> {
    let mut keyed = LanguageKeyed::<BTreeSet<String>>::default();
    for selector in selectors {
        keyed.get_mut(selector.language).insert(selector.id.clone());
    }
    keyed.map(|ids| ids.into_iter().collect())
}

#[cfg(test)]
fn selectors_by_language(selectors: &[TestSelector]) -> (Vec<String>, Vec<String>) {
    let keyed = keyed_selectors(selectors);
    (keyed.python, keyed.rust)
}

pub(crate) fn split_source_paths(source_paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    source_paths
        .iter()
        .cloned()
        .partition(|path| !super::is_rust_planning_source_path(path))
}

#[cfg(test)]
#[path = "selection_rss_test.rs"]
mod selection_rss_test;
#[cfg(test)]
#[path = "decision_test.rs"]
mod tests;
