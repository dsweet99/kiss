use kiss::Language;

use super::runners;
use super::{PlannedSelectors, planned_from_selector_plan};
use crate::test_git::TestChangeMode;

pub(crate) struct PlanSelectorsRequest<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub ignore: &'a [String],
    pub extras: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
}

pub(crate) struct VcsWorkspace {
    pub repo_root: std::path::PathBuf,
    pub ignore_norm: Vec<String>,
    pub source_changed: Vec<std::path::PathBuf>,
    pub test_changed: Vec<std::path::PathBuf>,
}

pub(crate) fn plan_vcs_workspace(req: &PlanSelectorsRequest<'_>) -> Result<VcsWorkspace, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    plan_vcs_workspace_at(req, repo_root)
}

pub(crate) fn plan_vcs_workspace_at(
    req: &PlanSelectorsRequest<'_>,
    repo_root: std::path::PathBuf,
) -> Result<VcsWorkspace, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(req.ignore);
    let diff_target = crate::test_git::resolve_diff_target(
        &repo_root,
        req.mode,
        req.config_main_branch,
        req.main_branch_cli,
        req.base_branch_cli,
    )?;
    let rel_changed = match req.mode {
        TestChangeMode::Commit => crate::test_git::changed_paths_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            crate::test_git::changed_paths_since(&repo_root, diff_target.as_ref().unwrap())?
        }
    };
    let lang_filter = req.lang_filter.map(crate::test_git::TestLangFilter::from);
    if let Some(language) = req.lang_filter {
        crate::test_runner::lang_registry::rules_for(language)
            .validate_extra_args(req.extras.get(language))?;
    }
    let abs_paths = crate::test_git::resolve_changed_source_paths(
        &repo_root,
        &rel_changed,
        &ignore_norm,
        lang_filter,
    );
    let existing_changed: Vec<_> = abs_paths
        .iter()
        .filter(|path| path.exists())
        .cloned()
        .collect();
    let roles = runners::roles_for_changed_paths(&existing_changed)
        .map_err(|err| format!("error: kiss test: {err}"))?;
    let (source_changed, test_changed) =
        runners::partition_changed_paths_with_roles(&abs_paths, &roles);
    Ok(VcsWorkspace {
        repo_root,
        ignore_norm,
        source_changed,
        test_changed,
    })
}

pub(crate) fn plan_selectors_from_workspace(
    ws: &VcsWorkspace,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let selector_plan = runners::combined_selectors_with_direct(runners::CombinedSelectorInput {
        repo_root: &ws.repo_root,
        source_paths: &ws.source_changed,
        test_paths: &ws.test_changed,
        test_args: extras,
        lang_filter,
        ignore: &ws.ignore_norm,
        extra_direct: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        include_prior_failures: true,
    })?;
    Ok(planned_from_selector_plan(
        ws.repo_root.clone(),
        selector_plan,
        ws.ignore_norm.clone(),
    ))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn plan_selectors(req: PlanSelectorsRequest<'_>) -> Result<PlannedSelectors, String> {
    let ws = plan_vcs_workspace(&req)?;
    plan_selectors_from_workspace(&ws, req.extras, req.lang_filter)
}
