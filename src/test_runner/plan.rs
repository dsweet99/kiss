use kiss::Language;

use super::language_keyed::LanguageKeyed;
use super::{
    PlannedSelectors, runners,
    targets::{ExpandedTargetPlan, expand_target_operands},
};

#[path = "plan_explicit.rs"]
mod plan_explicit;
use plan_explicit::plan_explicit_target_selectors;

#[path = "plan_vcs.rs"]
mod plan_vcs;
#[cfg(test)]
pub(crate) use plan_vcs::plan_selectors;
pub(crate) use plan_vcs::{
    PlanSelectorsRequest, VcsWorkspace, plan_selectors_from_workspace, plan_vcs_workspace_at,
};

pub(crate) enum TargetPlanKind<'a> {
    #[allow(dead_code)]
    All,
    Targets(&'a [String]),
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn plan_target_selectors(
    kind: TargetPlanKind<'_>,
    ignore: &[String],
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
    gate: &kiss::GateConfig,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    if let Some(language) = lang_filter {
        super::lang_registry::rules_for(language).validate_extra_args(extras.get(language))?;
    }
    match kind {
        TargetPlanKind::All => {
            plan_all_selectors(&repo_root, &ignore_norm, extras, lang_filter, gate)
        }
        TargetPlanKind::Targets(targets) => {
            match expand_target_operands(&repo_root, targets, &ignore_norm, lang_filter)
                .map_err(|e| format!("error: kiss test: {e}"))?
            {
                ExpandedTargetPlan::All => {
                    plan_all_selectors(&repo_root, &ignore_norm, extras, lang_filter, gate)
                }
                ExpandedTargetPlan::Files(files) if files.paths.is_empty() => Ok(
                    super::planned_selectors::empty_planned(repo_root.clone(), ignore_norm),
                ),
                ExpandedTargetPlan::Files(files) => plan_explicit_target_selectors(
                    &repo_root,
                    &files.paths,
                    &files.skip_python_collect,
                    &ignore_norm,
                    extras,
                    lang_filter,
                ),
            }
        }
    }
}

fn plan_all_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
    gate: &kiss::GateConfig,
) -> Result<PlannedSelectors, String> {
    if let Some(planned) = try_plan_all_from_cache(repo_root, ignore, extras, lang_filter, gate) {
        return Ok(planned);
    }
    let sel = discover_all_selectors(repo_root, ignore, extras, lang_filter)?;
    let fp = if lang_filter.is_none() {
        super::workspace_selector_cache::store_workspace_selector_sets(
            repo_root, ignore, &sel, extras,
        )
    } else {
        None
    };
    Ok(planned_all(repo_root, ignore, extras, sel, fp, gate))
}

pub(crate) struct AllWorkspaceCache {
    pub sel: LanguageKeyed<Vec<String>>,
    pub fp: String,
}

pub(crate) fn load_all_workspace_cache(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
) -> Option<AllWorkspaceCache> {
    let cache_started = std::time::Instant::now();
    let (cached, fp) = super::workspace_selector_cache::load_cached_workspace_selector_sets(
        repo_root,
        ignore,
        extras,
        lang_filter,
    )?;
    let sel = keep_allowed(lang_filter, cached);
    crate::test_runner::emit_stage_time("plan_cache", cache_started.elapsed());
    Some(AllWorkspaceCache { sel, fp })
}

pub(crate) fn select_all_language(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    language: Language,
    gate: &kiss::GateConfig,
    cached: Option<&AllWorkspaceCache>,
) -> Result<PlannedSelectors, String> {
    let (sel, fp) = match cached {
        Some(cache) => (cache.sel.clone(), Some(cache.fp.clone())),
        None => {
            let (selectors, elapsed) = timed_selectors(repo_root, ignore, extras, language);
            emit_plan_stage(language, elapsed);
            let mut by_language = LanguageKeyed::<Vec<String>>::default();
            *by_language.get_mut(language) = selectors?;
            (by_language, None)
        }
    };
    let sel = keep_allowed(Some(language), sel);
    Ok(planned_all(repo_root, ignore, extras, sel, fp, gate))
}

fn try_plan_all_from_cache(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
    gate: &kiss::GateConfig,
) -> Option<PlannedSelectors> {
    let cache = load_all_workspace_cache(repo_root, ignore, extras, lang_filter)?;
    Some(planned_all(
        repo_root,
        ignore,
        extras,
        cache.sel,
        Some(cache.fp),
        gate,
    ))
}

fn keep_allowed(
    lang_filter: Option<Language>,
    mut selectors: LanguageKeyed<Vec<String>>,
) -> LanguageKeyed<Vec<String>> {
    for language in crate::test_runner::lang_registry::languages() {
        if !language.allowed_by(lang_filter) {
            selectors.get_mut(language).clear();
        }
    }
    selectors
}

fn emit_plan_stage(language: Language, elapsed: std::time::Duration) {
    crate::test_runner::emit_stage_time(&format!("plan_{}", language.label()), elapsed);
}

fn timed_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    language: Language,
) -> (Result<Vec<String>, String>, std::time::Duration) {
    let started = std::time::Instant::now();
    let out = super::lang_registry::rules_for(language).list_workspace_selectors(
        repo_root,
        ignore,
        extras.get(language),
    );
    (out, started.elapsed())
}

fn discover_all_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
) -> Result<LanguageKeyed<Vec<String>>, String> {
    let wanted: Vec<Language> = crate::test_runner::lang_registry::languages()
        .into_iter()
        .filter(|language| language.allowed_by(lang_filter))
        .collect();
    let mut found = LanguageKeyed::<Vec<String>>::default();
    if let [first, second] = wanted[..] {
        let (first_res, second_res) = rayon::join(
            || timed_selectors(repo_root, ignore, extras, first),
            || timed_selectors(repo_root, ignore, extras, second),
        );
        *found.get_mut(first) = first_res.0?;
        *found.get_mut(second) = second_res.0?;
        emit_plan_stage(first, first_res.1);
        emit_plan_stage(second, second_res.1);
    } else {
        for language in wanted {
            let (selectors, elapsed) = timed_selectors(repo_root, ignore, extras, language);
            emit_plan_stage(language, elapsed);
            *found.get_mut(language) = selectors?;
        }
    }
    Ok(found)
}

fn planned_all(
    repo_root: &std::path::Path,
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    mut sel: LanguageKeyed<Vec<String>>,
    workspace_files_fingerprint: Option<String>,
    gate: &kiss::GateConfig,
) -> PlannedSelectors {
    let plans = LanguageKeyed::from_fn(|language| {
        super::lang_registry::rules_for(language).all_mode_plan(
            repo_root,
            extras.get(language),
            std::mem::take(sel.get_mut(language)),
            gate,
        )
    });
    let population_required =
        LanguageKeyed::from_fn(|language| plans.get(language).population_required);
    let sel = plans.map(|plan| plan.planned);
    planned_current(
        repo_root,
        ignore,
        sel,
        population_required,
        workspace_files_fingerprint,
    )
}

fn planned_current(
    repo_root: &std::path::Path,
    ignore: &[String],
    sel: LanguageKeyed<Vec<String>>,
    population_required: LanguageKeyed<bool>,
    workspace_files_fingerprint: Option<String>,
) -> PlannedSelectors {
    PlannedSelectors {
        repo_root: repo_root.to_path_buf(),
        sel,
        population_required,
        source_paths: LanguageKeyed::default(),
        vcs_source_paths: LanguageKeyed::default(),
        prior_failure_selectors: LanguageKeyed::default(),
        selection_engine_used: false,
        selection_basis: LanguageKeyed::from_fn(|_| {
            crate::test_runner::test_selection::SelectionBasis::Current
        }),
        ignore: ignore.to_vec(),
        workspace_files_fingerprint,
        skip_index_rebuild_after_selective: LanguageKeyed::from_fn(|language| {
            super::lang_registry::rules_for(language).all_mode_skips_index_rebuild()
        }),
    }
}

pub(super) fn planned_from_selector_plan(
    repo_root: std::path::PathBuf,
    selector_plan: crate::test_runner::runners::SelectorPlan,
    ignore: Vec<String>,
) -> PlannedSelectors {
    PlannedSelectors {
        repo_root,
        sel: selector_plan.selectors,
        population_required: selector_plan.population_required,
        source_paths: selector_plan.source_paths,
        vcs_source_paths: selector_plan.vcs_source_paths,
        prior_failure_selectors: selector_plan.prior_failure_selectors,
        selection_engine_used: selector_plan.selection_engine_used,
        selection_basis: selector_plan.selection_basis,
        ignore,
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: LanguageKeyed::default(),
    }
}

#[cfg(test)]
#[path = "plan_cold_test.rs"]
mod plan_cold_test;
