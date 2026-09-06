#[path = "pipeline_jobs.rs"]
mod pipeline_jobs;
#[cfg(test)]
pub(crate) use pipeline_jobs::{
    COVERING_HOOKS, CoveringHooks, EXECUTE_HOOKS, ExecuteHooks, STUB_LANGUAGE_EXECUTE,
    set_blocked_covering_language, set_fail_covering, unpark_blocked_covering,
};

use std::time::Instant;

use kiss::Language;

use super::RunTestCmdArgs;
use super::plan::{
    AllWorkspaceCache, PlanSelectorsRequest, TargetPlanKind, VcsWorkspace, cover_all_language,
    plan_selectors_from_workspace, plan_target_selectors_with_priors, plan_vcs_workspace_at,
};
use super::planned_selectors::{
    PlannedSelectors, SelectorRunOptions, should_force_cold_initialization,
};
use super::run_logic::{finish_joined_run, merge_language_planned, print_joined_dry_run};
use crate::bin_cli::args::TestInvocation;
use crate::test_git::TestChangeMode;

enum SharedKind {
    Change(VcsWorkspace),
    All { cache: Option<AllWorkspaceCache> },
    Targets,
}

pub(super) struct SharedPrefix {
    pub(super) repo_root: std::path::PathBuf,
    pub(super) ignore: Vec<String>,
    kind: SharedKind,
    pub(super) python_may_work: bool,
    pub(super) rust_may_work: bool,
    pub(super) cold_init: bool,
}

pub(crate) fn run_overlapped_test(
    a: &RunTestCmdArgs<'_>,
    process_started: Instant,
) -> Result<i32, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let session_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let _inventory_session =
        super::workspace_selector_cache::begin_inventory_session(&session_root);
    let prefix = run_workspace_prefix(a, &session_root)?;
    let slots = pipeline_jobs::LanguageSlots::default();
    pipeline_jobs::spawn_language_jobs(a, &prefix, &slots)?;
    let (python_job, rust_job) = pipeline_jobs::take_job_results(&slots)?;
    let planned = merge_and_cache_planned(a, &prefix, &slots)?;
    let options = run_options(a, a.jobs, process_started);
    if a.dry_run {
        print_joined_dry_run(&planned, &options)?;
        return Ok(0);
    }
    finish_joined_run(&planned, &options, process_started, python_job, rust_job)
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
    let (python, rust) = pipeline_jobs::take_planned(slots);
    let mut planned = merge_language_planned(
        prefix.repo_root.clone(),
        prefix.ignore.clone(),
        python,
        rust,
    );
    if matches!(a.invocation, TestInvocation::All)
        && a.lang_filter.is_none()
        && planned.workspace_files_fingerprint.is_none()
        && (!planned.sel.python.is_empty() || !planned.sel.rust.is_empty())
    {
        planned.workspace_files_fingerprint =
            crate::test_runner::workspace_selector_cache::store_workspace_selectors(
                &prefix.repo_root,
                &prefix.ignore,
                &planned.sel.python,
                &planned.sel.rust,
                a.python_extra,
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
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: a.python_extra,
            rust: a.extra,
        },
        plan_duration: process_started.elapsed(),
        gate: a.gate_config.clone(),
    }
}

fn plan_shared_prefix(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
) -> Result<SharedPrefix, String> {
    match &a.invocation {
        TestInvocation::Commit | TestInvocation::Base | TestInvocation::Main => {
            let req = change_request(a);
            let ws = plan_vcs_workspace_at(&req, repo_root.to_path_buf())?;
            let cold_init = should_force_cold_initialization(a, &ws.repo_root);
            let python_may_work = a.lang_filter != Some(Language::Rust)
                && language_thread_may_work(&ws, Language::Python, cold_init)?;
            let rust_may_work = a.lang_filter != Some(Language::Python)
                && language_thread_may_work(&ws, Language::Rust, cold_init)?;
            Ok(SharedPrefix {
                repo_root: ws.repo_root.clone(),
                ignore: ws.ignore_norm.clone(),
                kind: SharedKind::Change(ws),
                python_may_work,
                rust_may_work,
                cold_init,
            })
        }
        TestInvocation::All => plan_all_or_targets_prefix(a, repo_root, None),
        TestInvocation::Targets(targets) => plan_all_or_targets_prefix(a, repo_root, Some(targets)),
    }
}

fn plan_all_or_targets_prefix(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
    targets: Option<&[String]>,
) -> Result<SharedPrefix, String> {
    let ignore = kiss::normalize_ignore_prefixes(a.ignore);
    if matches!(a.lang_filter, Some(Language::Rust)) {
        super::rust_llvm_cov::validate_rust_extra_args(a.extra)?;
    }
    let kind = if targets.is_none() {
        SharedKind::All {
            cache: super::plan::load_all_workspace_cache(
                repo_root,
                &ignore,
                a.python_extra,
                a.lang_filter,
            ),
        }
    } else {
        SharedKind::Targets
    };
    let python_has_cached_work =
        !matches!(&kind, SharedKind::All { cache: Some(cache) } if cache.py.is_empty());
    let rust_has_cached_work =
        !matches!(&kind, SharedKind::All { cache: Some(cache) } if cache.rs.is_empty());
    let cold_init = should_force_cold_initialization(a, repo_root);
    Ok(SharedPrefix {
        python_may_work: a.lang_filter != Some(Language::Rust) && python_has_cached_work,
        rust_may_work: a.lang_filter != Some(Language::Python) && rust_has_cached_work,
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
        priors: language_has_prior_failure_records(&ws.repo_root, language)?,
        cold_init,
    }
    .yes())
}

fn language_has_prior_failure_records(
    repo_root: &std::path::Path,
    language: Language,
) -> Result<bool, String> {
    crate::test_runner::last_status::has_language_records(repo_root, language)
}

fn language_paths_may_work(ws: &VcsWorkspace, language: Language) -> bool {
    let ext = match language {
        Language::Python => "py",
        Language::Rust => "rs",
    };
    ws.source_changed
        .iter()
        .chain(ws.test_changed.iter())
        .any(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
}

pub(super) fn cover_language(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    language: Language,
) -> Result<PlannedSelectors, String> {
    if pipeline_jobs::covering_should_fail(language) {
        return Err("error: kiss test: covering failed".to_string());
    }
    let extras = crate::test_runner::language_keyed::LanguageKeyed {
        python: a.python_extra,
        rust: a.extra,
    };
    match &prefix.kind {
        SharedKind::Change(ws) => plan_selectors_from_workspace(ws, extras, Some(language)),
        SharedKind::All { cache } => cover_all_language(
            &prefix.repo_root,
            &prefix.ignore,
            a.python_extra,
            language,
            &a.gate_config,
            cache.as_ref(),
        ),
        SharedKind::Targets => {
            let TestInvocation::Targets(targets) = &a.invocation else {
                return Err("error: kiss test: missing targets".to_string());
            };
            let thread_targets = cover_thread_targets(targets, language, a.lang_filter)?;
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
    let mode = match a.invocation {
        TestInvocation::Commit => TestChangeMode::Commit,
        TestInvocation::Base => TestChangeMode::Base,
        TestInvocation::Main => TestChangeMode::Main,
        TestInvocation::All | TestInvocation::Targets(_) => TestChangeMode::Commit,
    };
    PlanSelectorsRequest {
        mode,
        main_branch_cli: a.main_branch_cli,
        base_branch_cli: a.base_branch_cli,
        ignore: a.ignore,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: a.python_extra,
            rust: a.extra,
        },
        lang_filter: a.lang_filter,
        config_main_branch: a.config_main_branch,
    }
}

pub(crate) fn split_jobs(jobs: usize, both: bool) -> (usize, usize) {
    let full = jobs.max(1);
    if both {
        let half = (full / 2).max(1);
        (half, half)
    } else {
        (full, full)
    }
}

fn cover_thread_targets(
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
    match std::path::Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
    {
        Some(ext) if ext.eq_ignore_ascii_case("py") => Some(Language::Python),
        Some(ext) if ext.eq_ignore_ascii_case("rs") => Some(Language::Rust),
        _ => None,
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::{LanguageMayWork, VcsWorkspace, language_paths_may_work, split_jobs};
    use kiss::Language;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn jobs_split_both_languages_and_lang_filter() {
        assert_eq!(split_jobs(4, true), (2, 2));
        assert_eq!(split_jobs(4, false), (4, 4));
        assert_eq!(split_jobs(1, true), (1, 1));
    }

    #[test]
    fn configured_jobs_are_honored_without_a_hidden_cap() {
        assert_eq!(split_jobs(48, true), (24, 24));
        assert_eq!(split_jobs(48, false), (48, 48));
        assert_eq!(split_jobs(32, true), (16, 16));
        assert_eq!(split_jobs(16, false), (16, 16));
        assert_eq!(split_jobs(8, false), (8, 8));
        assert_eq!(split_jobs(0, false), (1, 1));
    }

    #[test]
    fn vcs_spawn_uses_paths_priors_and_cold_init() {
        assert!(
            !LanguageMayWork {
                paths: false,
                priors: false,
                cold_init: false
            }
            .yes()
        );
        assert!(
            LanguageMayWork {
                paths: true,
                priors: false,
                cold_init: false
            }
            .yes()
        );
        assert!(
            LanguageMayWork {
                paths: false,
                priors: true,
                cold_init: false
            }
            .yes()
        );
        assert!(
            LanguageMayWork {
                paths: false,
                priors: false,
                cold_init: true
            }
            .yes()
        );
        let ws = VcsWorkspace {
            repo_root: PathBuf::from("."),
            ignore_norm: Vec::new(),
            source_changed: vec![PathBuf::from("lib.py")],
            test_changed: Vec::new(),
            changed_lines: BTreeMap::new(),
        };
        assert!(language_paths_may_work(&ws, Language::Python));
        assert!(!language_paths_may_work(&ws, Language::Rust));
    }
}
