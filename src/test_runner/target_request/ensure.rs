use std::path::Path;

use super::report::{TargetPlanPreview, TargetReport};
use super::snapshot::{
    EnsureError, EnsurePolicy, SnapshotOutcome, run_snapshot_kernel, run_snapshot_kernel_with,
};
use super::types::TargetRequest;

#[derive(Debug)]
pub(crate) enum Ensured {
    Report(Box<TargetReport>),
}

#[derive(Debug)]
pub(crate) enum EnsureOutcome {
    Ready {
        report: Box<TargetReport>,
        replay: bool,
    },
    Interrupted {
        ready: Option<Box<TargetReport>>,
    },
    Miss {
        exit_code: i32,
        error: Option<String>,
        engine_aborted: bool,
        typed: bool,
    },
    /// The worktree changed while the tests ran, so the cache cannot prove a
    /// report for it. The run's own result stands; the edit gets its own run.
    Moved {
        exit_code: i32,
    },
}

pub(crate) fn ensure_target_report<F>(
    repo_root: Option<&Path>,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
    reuse: bool,
    close_zero: bool,
    execute: F,
) -> EnsureOutcome
where
    F: FnMut(crate::test_runner::RunTestCmdArgs<'_>) -> crate::test_runner::RunTestOnceOutcome,
{
    ignore_legacy_suite_report(repo_root);
    if reuse && let Some(report) = ready_report(repo_root, args, true) {
        return EnsureOutcome::Ready {
            report: Box::new(report),
            replay: true,
        };
    }
    execute_then_publish(repo_root, args, close_zero, execute)
}

#[cfg(test)]
pub(crate) fn materialize_target_report(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
) -> Result<TargetReport, EnsureError> {
    match ensure_target_report_with(repo_root, request, policy, None)? {
        Ensured::Report(report) => Ok(*report),
    }
}

pub(crate) fn ensure_target_report_query(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Result<Ensured, EnsureError> {
    if let Some(ready) =
        super::bind::load_ready_for_request(repo_root, request, policy.coverage_all(), extras)
    {
        return Ok(Ensured::Report(Box::new(ready)));
    }
    if let Some(projected) = super::bind::project_language_ready_from_parent_workspace(
        repo_root,
        request,
        policy.coverage_all(),
        extras,
    ) {
        return Ok(Ensured::Report(Box::new(projected)));
    }
    if let Some(projected) = super::bind::project_git_ready_from_parent_workspace(
        repo_root,
        request,
        policy.coverage_all(),
        extras,
    ) {
        return Ok(Ensured::Report(Box::new(projected)));
    }
    Err(EnsureError::IncompleteEvidence(
        "incomplete evidence".into(),
    ))
}

pub(crate) fn assemble_target_report_query(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Result<Ensured, EnsureError> {
    if let Ok(ready) = ensure_target_report_query(repo_root, request, policy, extras) {
        return Ok(ready);
    }
    match run_snapshot_kernel(repo_root, request, policy)? {
        SnapshotOutcome::Report(report) => Ok(Ensured::Report(report)),
        SnapshotOutcome::Preview(_) => Err(EnsureError::Planning(
            "dry-run produced a preview instead of a report".into(),
        )),
    }
}

pub(crate) fn ensure_target_report_with(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<Ensured, EnsureError> {
    let pending = crate::test_runner::ensure_runtime::deferred_edit_pending();
    if !policy.dry_run()
        && !policy.retry_bad()
        && !args.is_some_and(|item| item.force_rerun)
        && !pending
    {
        let extras = args
            .map(|item| item.extras)
            .unwrap_or(crate::test_runner::language_keyed::LanguageKeyed::EMPTY);
        if let Ok(ensured) = ensure_target_report_query(repo_root, request, policy, extras) {
            return Ok(ensured);
        }
        if args.is_none() && policy.require_complete() {
            return Err(EnsureError::IncompleteEvidence(
                "incomplete evidence".into(),
            ));
        }
    }
    match run_snapshot_kernel_with(repo_root, request, policy, args)? {
        SnapshotOutcome::Report(report) => Ok(Ensured::Report(report)),
        SnapshotOutcome::Preview(_) => Err(EnsureError::Planning(
            "dry-run produced a preview instead of a report".into(),
        )),
    }
}

pub(crate) fn preview_target_plan_with(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
) -> Result<TargetPlanPreview, EnsureError> {
    let policy = EnsurePolicy::preview(policy.retry_bad(), policy.coverage_all());
    match run_snapshot_kernel(repo_root, request, &policy)? {
        SnapshotOutcome::Preview(preview) => Ok(preview),
        SnapshotOutcome::Report(_) => {
            Err(EnsureError::Planning("expected a dry-run preview".into()))
        }
    }
}

fn ready_report(
    repo_root: Option<&Path>,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
    refuse_force: bool,
) -> Option<TargetReport> {
    if args.dry_run || (refuse_force && (args.force_rerun || args.force_bad)) {
        return None;
    }
    let repo = repo_root?;
    match ensure_target_report_query(
        repo,
        &super::request_from_run_args(args),
        &EnsurePolicy::query(args.coverage_all),
        args.extras,
    ) {
        Ok(Ensured::Report(report)) => Some(*report),
        Err(_) => None,
    }
}

fn execute_then_publish<F>(
    repo_root: Option<&Path>,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
    close_zero: bool,
    mut execute: F,
) -> EnsureOutcome
where
    F: FnMut(crate::test_runner::RunTestCmdArgs<'_>) -> crate::test_runner::RunTestOnceOutcome,
{
    kiss::rust_llvm_cov_runner::begin_watch_report_capture();
    let defer = crate::test_runner::final_summary::RecapDeferGuard::enter();
    let worktree_before = worktree_token(repo_root, args);
    let mut published = None;
    let ran = match execute(crate::test_runner::clone_run_args(args)) {
        crate::test_runner::RunTestOnceOutcome::Interrupted => Executed::Interrupted,
        crate::test_runner::RunTestOnceOutcome::EngineError(msg) => Executed::Engine(msg),
        crate::test_runner::RunTestOnceOutcome::Code(code) => {
            if !args.dry_run {
                published = ready_report(repo_root, args, false).filter(|report| {
                    TargetReport::combine_exit(report.exit_code, code) == report.exit_code
                });
                if published.is_none() {
                    super::remember_named(args, code, repo_root);
                }
            }
            Executed::Code(code)
        }
    };
    let published = published.or_else(|| ready_report(repo_root, args, false));
    crate::test_runner::lang_python::generation::clear_python_execution_identity_memo();
    if published.is_some() {
        defer.discard();
    } else if let Executed::Code(code) = &ran
        && worktree_token(repo_root, args) != worktree_before
    {
        return EnsureOutcome::Moved { exit_code: *code };
    }
    finish_executed(repo_root, args, close_zero, ran, published)
}

fn worktree_token(
    repo_root: Option<&Path>,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Option<String> {
    if args.dry_run {
        return None;
    }
    repo_root.map(|repo| super::stamp::capture_worktree_token(repo, args.lang_filter()))
}

enum Executed {
    Interrupted,
    Engine(String),
    Code(i32),
}

fn finish_executed(
    repo_root: Option<&Path>,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
    close_zero: bool,
    ran: Executed,
    published: Option<TargetReport>,
) -> EnsureOutcome {
    let assemble = crate::test_runner::repo_can_assemble_reports(repo_root);
    let typed = assemble && !args.dry_run;
    match ran {
        Executed::Interrupted => EnsureOutcome::Interrupted {
            ready: ready_report(repo_root, args, true).map(Box::new),
        },
        Executed::Engine(msg) => published_or_miss(published, 1, Some(msg), true, typed),
        Executed::Code(code) => {
            if code == 0 && assemble && published.is_none() && (close_zero || !args.dry_run) {
                return EnsureOutcome::Miss {
                    exit_code: 1,
                    error: Some("target membership is not proven complete".into()),
                    engine_aborted: false,
                    typed: true,
                };
            }
            published_or_miss(published, code, None, false, typed)
        }
    }
}

fn published_or_miss(
    published: Option<TargetReport>,
    exit_code: i32,
    error: Option<String>,
    engine_aborted: bool,
    typed: bool,
) -> EnsureOutcome {
    match published {
        Some(report) => EnsureOutcome::Ready {
            report: Box::new(report),
            replay: false,
        },
        None => EnsureOutcome::Miss {
            exit_code,
            error,
            engine_aborted,
            typed,
        },
    }
}

fn ignore_legacy_suite_report(repo_root: Option<&Path>) {
    let owned;
    let repo = match repo_root {
        Some(path) => path,
        None => {
            let Ok(cwd) = std::env::current_dir() else {
                return;
            };
            owned = match crate::test_git::git_repo_root(&cwd) {
                Ok(path) => path,
                Err(_) => return,
            };
            &owned
        }
    };
    let _ = std::fs::remove_file(repo.join(".kiss").join("suite_report.json"));
}
