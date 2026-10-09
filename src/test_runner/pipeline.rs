#[path = "pipeline_jobs.rs"]
mod pipeline_jobs;
pub(crate) use pipeline_jobs::PipelineDoubles;

use std::time::Instant;

use kiss::Language;

use super::RunTestCmdArgs;
use super::language_keyed::LanguageKeyed;
use super::plan::{
    AllWorkspaceCache, PlanSelectorsRequest, TargetPlanKind, VcsWorkspace,
    plan_selectors_from_workspace, plan_target_selectors_with_priors, plan_vcs_workspace_at,
    select_all_language,
};
use super::planned_selectors::{
    PlannedSelectors, SelectorRunOptions, should_force_cold_initialization,
};
use super::run_logic::{finish_joined_run, merge_language_planned, print_joined_dry_run};
use crate::test_runner::target_request::{
    TargetFocus, change_mode_from_focus, operand_raws, request_from_run_args,
};

enum SharedKind {
    Change(VcsWorkspace),
    All { cache: Option<AllWorkspaceCache> },
    Targets(Vec<String>),
}

pub(super) struct SharedPrefix {
    pub(super) repo_root: std::path::PathBuf,
    pub(super) ignore: Vec<String>,
    kind: SharedKind,
    pub(super) may_work: LanguageKeyed<bool>,
    pub(super) cold_init: bool,
}

pub(crate) fn run_overlapped_test(
    a: &RunTestCmdArgs<'_>,
    process_started: Instant,
) -> Result<i32, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let session_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    for rules in crate::test_runner::lang_registry::all_rules() {
        rules.reclaim_unreferenced(&session_root);
    }
    let _inventory_session =
        super::workspace_selector_cache::begin_inventory_session(&session_root);
    let prefix = run_workspace_prefix(a, &session_root)?;
    let slots = pipeline_jobs::LanguageSlots::default();
    pipeline_jobs::spawn_language_jobs(a, &prefix, &slots)?;
    let jobs = pipeline_jobs::take_job_results(&slots)?;
    let planned = merge_and_cache_planned(a, &prefix, &slots)?;
    let options = run_options(a, a.jobs, process_started);
    if a.dry_run && !joined_has_work(&planned) {
        return finish_joined_run(&planned, &options, process_started, jobs);
    }
    if a.dry_run {
        print_joined_dry_run(&planned, &options)?;
        return Ok(0);
    }
    finish_joined_run(&planned, &options, process_started, jobs)
}

fn joined_has_work(planned: &PlannedSelectors) -> bool {
    Language::ALL
        .into_iter()
        .any(|language| super::run_logic::language_has_work(planned, language))
}

fn run_workspace_prefix(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
) -> Result<SharedPrefix, String> {
    super::emit_test_progress("kiss test: Running workspace");
    let workspace_started = Instant::now();
    let prefix = plan_shared_prefix(a, repo_root)?;
    super::emit_test_progress(&format!(
        "kiss test: Ran workspace {}ms",
        workspace_started.elapsed().as_millis()
    ));
    Ok(prefix)
}

fn merge_and_cache_planned(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    slots: &pipeline_jobs::LanguageSlots,
) -> Result<PlannedSelectors, String> {
    let mut planned = merge_language_planned(
        prefix.repo_root.clone(),
        prefix.ignore.clone(),
        pipeline_jobs::take_planned(slots),
    );
    if crate::test_runner::target_request::is_workspace_run(a)
        && a.lang_filter().is_none()
        && planned.workspace_files_fingerprint.is_none()
        && !planned.sel.both_empty()
    {
        planned.workspace_files_fingerprint =
            crate::test_runner::workspace_selector_cache::store_workspace_selector_sets(
                &prefix.repo_root,
                &prefix.ignore,
                &planned.sel,
                a.extras,
            );
    }
    Ok(planned)
}

fn run_options<'a>(
    a: &'a RunTestCmdArgs<'a>,
    jobs: usize,
    process_started: Instant,
) -> SelectorRunOptions<'a> {
    SelectorRunOptions {
        dry_run: a.dry_run,
        force_rerun: a.force_rerun,
        metrics: a.metrics,
        jobs,
        extras: a.extras,
        plan_duration: process_started.elapsed(),
        gate: a.gate_config.clone(),
    }
}

fn plan_shared_prefix(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
) -> Result<SharedPrefix, String> {
    let request = request_from_run_args(a);
    match &request.focus {
        TargetFocus::Git(_) => {
            let req = change_request(a);
            let ws = plan_vcs_workspace_at(&req, repo_root.to_path_buf())?;
            let cold_init = should_force_cold_initialization(a, &ws.repo_root);
            let mut may_work = LanguageKeyed::<bool>::default();
            for language in crate::test_runner::lang_registry::languages() {
                *may_work.get_mut(language) = language.allowed_by(a.lang_filter())
                    && language_thread_may_work(&ws, language, cold_init)?;
            }
            Ok(SharedPrefix {
                repo_root: ws.repo_root.clone(),
                ignore: ws.ignore_norm.clone(),
                kind: SharedKind::Change(ws),
                may_work,
                cold_init,
            })
        }
        TargetFocus::Workspace => plan_all_or_targets_prefix(a, repo_root, None),
        TargetFocus::Operands(_) => {
            let targets = operand_raws(&request.focus).unwrap_or_default();
            plan_all_or_targets_prefix(a, repo_root, Some(&targets))
        }
    }
}

fn plan_all_or_targets_prefix(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
    targets: Option<&[String]>,
) -> Result<SharedPrefix, String> {
    let ignore = kiss::normalize_ignore_prefixes(a.ignore());
    if let Some(language) = a.lang_filter() {
        super::lang_registry::rules_for(language).validate_extra_args(a.extras.get(language))?;
    }
    let kind = match targets {
        None => SharedKind::All {
            cache: super::plan::load_all_workspace_cache(
                repo_root,
                &ignore,
                a.extras,
                a.lang_filter(),
            ),
        },
        Some(targets) => SharedKind::Targets(targets.to_vec()),
    };
    let has_cached_work = LanguageKeyed::from_fn(|language| match &kind {
        SharedKind::All { cache: Some(cache) } => !cache.sel.get(language).is_empty(),
        _ => true,
    });
    let cold_init = should_force_cold_initialization(a, repo_root);
    let mut may_work = has_cached_work;
    for language in crate::test_runner::lang_registry::languages() {
        *may_work.get_mut(language) &= language.allowed_by(a.lang_filter());
    }
    Ok(SharedPrefix {
        may_work,
        repo_root: repo_root.to_path_buf(),
        ignore,
        kind,
        cold_init,
    })
}

struct LanguageMayWork {
    paths: bool,
    priors: bool,
    cold_init: bool,
}

impl LanguageMayWork {
    fn yes(self) -> bool {
        self.cold_init || self.paths || self.priors
    }
}

fn language_thread_may_work(
    ws: &VcsWorkspace,
    language: Language,
    cold_init: bool,
) -> Result<bool, String> {
    Ok(LanguageMayWork {
        paths: language_paths_may_work(ws, language),
        priors: !kiss::test_records::nonpassed_test_ids(&kiss::test_records::records_dir(
            &ws.repo_root,
            language.label(),
        ))
        .is_empty(),
        cold_init,
    }
    .yes())
}

fn language_paths_may_work(ws: &VcsWorkspace, language: Language) -> bool {
    let ext = language.extension();
    ws.source_changed
        .iter()
        .chain(ws.test_changed.iter())
        .any(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
}

pub(super) fn select_language(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    language: Language,
) -> Result<PlannedSelectors, String> {
    if pipeline_jobs::selecting_should_fail(a, language) {
        return Err("error: kiss test: selecting failed".to_string());
    }
    let extras = a.extras;
    match &prefix.kind {
        SharedKind::Change(ws) => plan_selectors_from_workspace(ws, extras, Some(language)),
        SharedKind::All { cache } => select_all_language(
            &prefix.repo_root,
            &prefix.ignore,
            extras,
            language,
            &a.gate_config,
            cache.as_ref(),
        ),
        SharedKind::Targets(targets) => {
            let thread_targets = select_thread_targets(targets, language, a.lang_filter())?;
            if thread_targets.is_empty() {
                return Ok(super::empty_planned(
                    prefix.repo_root.clone(),
                    prefix.ignore.clone(),
                ));
            }
            plan_target_selectors_with_priors(
                TargetPlanKind::Targets(thread_targets.as_slice()),
                &prefix.ignore,
                extras,
                Some(language),
                &a.gate_config,
                a.force_bad,
            )
        }
    }
}

fn change_request<'a>(a: &'a RunTestCmdArgs<'a>) -> PlanSelectorsRequest<'a> {
    let mode = change_mode_from_focus(&request_from_run_args(a).focus);
    PlanSelectorsRequest {
        mode,
        main_branch_cli: a.main_branch_cli,
        base_branch_cli: a.base_branch_cli,
        ignore: a.ignore(),
        extras: a.extras,
        lang_filter: a.lang_filter(),
        config_main_branch: a.config_main_branch,
    }
}

fn select_thread_targets(
    targets: &[String],
    language: Language,
    user_lang: Option<Language>,
) -> Result<Vec<String>, String> {
    reject_user_lang_on_targets(targets, user_lang)?;
    Ok(targets
        .iter()
        .filter(|raw| target_belongs_to_thread(raw, language))
        .cloned()
        .collect())
}

fn reject_user_lang_on_targets(
    targets: &[String],
    user_lang: Option<Language>,
) -> Result<(), String> {
    let Some(filter) = user_lang else {
        return Ok(());
    };
    for raw in targets {
        let Some(language) = operand_source_language(raw) else {
            continue;
        };
        if language != filter {
            return Err(format!(
                "error: kiss test: target '{raw}' is {} but --lang selects only {}",
                language.label(),
                filter.label()
            ));
        }
    }
    Ok(())
}

fn target_belongs_to_thread(raw: &str, language: Language) -> bool {
    operand_source_language(raw).is_none_or(|operand| operand == language)
}

fn operand_source_language(raw: &str) -> Option<Language> {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    std::path::Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(crate::test_runner::lang_registry::language_for_extension)
}

#[cfg(test)]
#[path = "pipeline_unit_test.rs"]
mod pipeline_tests;
