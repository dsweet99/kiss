use super::ensure::{Ensured, ensure_target_report_with, preview_target_plan_with};
use super::report::TargetReport;
use super::report_store;
use super::snapshot::{EnsureError, EnsurePolicy};
use super::{compat_matches, request_from_run_args, to_compat_invocation};

pub(crate) enum BindDecision {
    Finished(i32),
    Interrupted,
}

pub(crate) fn load_ready_for_request(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let (projection, complete) =
        super::projection::build_slice_projection(repo, request, &resolved);
    let stamp = super::slice::stamp_from_projection(&projection, complete);
    if stamp.complete {
        load_identity_report(repo, request, &stamp, coverage_all, extras)
    } else {
        None
    }
}

fn load_identity_report(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    stamp: &super::slice::TargetSliceStamp,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<super::report::TargetReport> {
    report_store::load_report_for_identity(
        repo,
        request,
        &stamp.digest,
        stamp.complete,
        coverage_all,
        extras,
    )
    .filter(ready_report)
    .filter(|report| report.snapshot.worktree == super::stamp::capture_worktree_token(repo))
    .filter(|report| report.snapshot.extras.as_slices() == extras)
}

pub(crate) fn bind_and_prepare(
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Result<BindDecision, String> {
    super::counters::reset();
    let request = request_from_run_args(args);
    if !adapter_holds(args, &request) {
        return Err("error: kiss test: target request adapter mismatch".into());
    }
    crate::test_runner::emit_test_progress("kiss test: Planning ...");
    let cwd = std::env::current_dir().map_err(|err| format!("error: kiss test: {err}"))?;
    let repo = crate::test_git::git_repo_root(&cwd)
        .map_err(|err| format!("error: kiss test requires a git repository ({err})"))?;
    if args.dry_run {
        if args.target_request.language() != Some(kiss::Language::Python) {
            crate::test_runner::rust_llvm_cov::validate_rust_extra_args(args.extras.rust)?;
        }
        match preview_target_plan_with(
            &repo,
            &request,
            &EnsurePolicy::preview(args.force_bad, args.coverage_all),
        ) {
            Ok(preview) => {
                super::render::render_plan_preview(&preview);
                super::render::render_preview_members(&preview);
                super::counters::emit();
                return Ok(BindDecision::Finished(0));
            }
            Err(err) => {
                let _ = err.exit_code();
                return Err(err.to_string());
            }
        }
    }
    match ensure_target_report_with(
        &repo,
        &request,
        &EnsurePolicy::complete(args.force_bad, args.coverage_all),
        Some(args),
    ) {
        Ok(Ensured::Report(report)) => {
            let executed = super::counters::current().subprocess > 0;
            let _ = report_store::publish_if_rows_hold(&repo, &request, &report);
            render_bound_report(&report, executed);
            super::counters::emit();
            Ok(BindDecision::Finished(report.exit_code))
        }
        Err(EnsureError::Interrupted) => Ok(BindDecision::Interrupted),
        Err(err) => Err(err.to_string()),
    }
}

fn adapter_holds(
    _args: &crate::test_runner::RunTestCmdArgs<'_>,
    request: &super::types::TargetRequest,
) -> bool {
    let compat = to_compat_invocation(request);
    if !compat_matches(request, &compat) {
        return false;
    }
    true
}

fn render_bound_report(report: &TargetReport, executed: bool) {
    if executed {
        for line in super::render::official_summary_text(report).lines() {
            crate::test_runner::emit_test_progress(line);
        }
        return;
    }
    super::render::render_official_report(report);
}

fn ready_report(report: &TargetReport) -> bool {
    report.stamp.complete && report.rows.len() == report.scope.selectors.len()
}
