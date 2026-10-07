use std::path::PathBuf;
use std::time::Duration;

use super::RunTestCmdArgs;
use super::target_request::{GitFocus, TargetFocus};

#[derive(Clone)]
pub(crate) struct PlannedSelectors {
    pub repo_root: PathBuf,
    pub sel: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub population_required: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    pub source_paths: crate::test_runner::language_keyed::LanguageKeyed<Vec<PathBuf>>,
    pub vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub selection_engine_used: bool,
    pub selection_basis: crate::test_runner::language_keyed::LanguageKeyed<
        crate::test_runner::test_selection::SelectionBasis,
    >,
    pub ignore: Vec<String>,
    pub workspace_files_fingerprint: Option<String>,
    pub skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed<bool>,
}

pub(crate) fn empty_planned(repo_root: PathBuf, ignore: Vec<String>) -> PlannedSelectors {
    use crate::test_runner::language_keyed::LanguageKeyed;
    PlannedSelectors {
        repo_root,
        sel: LanguageKeyed::default(),
        population_required: LanguageKeyed::default(),
        source_paths: LanguageKeyed::default(),
        vcs_source_paths: LanguageKeyed::default(),
        prior_failure_selectors: LanguageKeyed::default(),
        selection_engine_used: false,
        selection_basis: LanguageKeyed::from_fn(|_| {
            crate::test_runner::test_selection::SelectionBasis::Current
        }),
        ignore,
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: LanguageKeyed::default(),
    }
}

pub(crate) struct SelectorRunOptions<'a> {
    #[allow(dead_code)]
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extras: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub plan_duration: Duration,
    pub gate: kiss::GateConfig,
}

pub(crate) fn should_force_cold_initialization(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
) -> bool {
    matches!(
        super::target_request::request_from_run_args(a).focus,
        TargetFocus::Git(
            GitFocus::AutomaticBase
                | GitFocus::ExplicitBase { .. }
                | GitFocus::DefaultMain
                | GitFocus::ConfiguredMain { .. }
                | GitFocus::ExplicitMain { .. }
        )
    ) && !a.dry_run
        && !a.force_rerun
        && !a.metrics
        && !crate::test_runner::lang_registry::languages()
            .into_iter()
            .any(|language| {
                crate::test_runner::lang_registry::rules_for(language)
                    .extras_block_cold_population(a.extras.get(language))
            })
        && a.ignore().is_empty()
        && a.lang_filter().is_none()
        && !crate::test_runner::test_state_dir(repo_root).exists()
}

pub(crate) fn apply_cold_initialization_population(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
) {
    if !should_force_cold_initialization(a, &planned.repo_root) {
        return;
    }
    planned.population_required =
        crate::test_runner::language_keyed::LanguageKeyed::from_fn(|_| true);
}

pub(crate) fn apply_force_all_population(a: &RunTestCmdArgs<'_>, planned: &mut PlannedSelectors) {
    if !a.force_rerun {
        return;
    }
    if !matches!(
        super::target_request::request_from_run_args(a).focus,
        TargetFocus::Workspace
    ) {
        return;
    }
    for language in crate::test_runner::lang_registry::languages() {
        if language.allowed_by(a.lang_filter()) && !planned.sel.get(language).is_empty() {
            *planned.population_required.get_mut(language) = true;
        }
    }
}

#[cfg(test)]
#[path = "planned_selectors_test.rs"]
mod tests;
