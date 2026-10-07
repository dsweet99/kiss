use super::ensure::{Ensured, ensure_target_report_with, preview_target_plan_with};
use super::report::TargetReport;
use super::snapshot::{EnsureError, EnsurePolicy};
use super::{compat_matches, request_from_run_args, to_compat_invocation};

pub(crate) enum BindDecision {
    Finished(i32),
    Interrupted,
}

pub(crate) fn load_ready_for_request(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    load_ready(repo, request, extras, false)
}

/// Like [`load_ready_for_request`], but a complete scope with no members is ready:
/// the run that just finished already settled that nothing is in scope.
pub(crate) fn load_ready_after_run(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    load_ready(repo, request, extras, true)
}

fn load_ready(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
    empty_is_ready: bool,
) -> Option<TargetReport> {
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let (projection, complete) =
        super::projection::build_slice_projection(repo, request, &resolved);
    let stamp = super::slice::stamp_from_projection(&projection, complete);
    if !stamp.complete {
        return None;
    }
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors);
    let scope = super::scope::ReportScope::from_membership(
        projection.report_regions(),
        selectors,
        stamp.complete,
    );
    if scope.selectors.is_empty() && !empty_is_ready {
        return None;
    }
    derive_ready_report(repo, request, scope, stamp, extras)
}

/// The report for `scope`, computed from the per-test records, when every member
/// has a current record and nothing needs to run.
fn derive_ready_report(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    scope: super::scope::ReportScope,
    stamp: super::slice::TargetSliceStamp,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let rows = super::rows::rows_from_witnesses(repo, &scope, extras).ok()?;
    for language in crate::test_runner::lang_registry::languages() {
        let language_extras = *extras.get(language);
        let recorded = rows.iter().any(|row| row.language == language.label());
        if (recorded || !language_extras.is_empty())
            && !crate::test_runner::lang_registry::rules_for(language)
                .stored_witness_matches_extras(repo, language_extras)
        {
            return None;
        }
    }
    let time_gate_active = !kiss::GateConfig::load_for_repo(repo)
        .max_unit_test_seconds
        .is_empty();
    let graph_repair = super::report::graph_repair_needed(repo, &scope);
    let plan = super::rows::plan_from_available_rows_with(
        &scope,
        &rows,
        false,
        graph_repair,
        false,
        time_gate_active,
    );
    if !plan.known_execution_union().is_empty() || plan.population_repair || plan.graph_repair {
        return None;
    }
    super::rows::duration_evidence_holds(&rows, time_gate_active).ok()?;
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut report = TargetReport::assembled_in(repo, request, scope, rows, stamp, exit_code);
    report.snapshot.extras = extras.owned_vecs();
    Some(report).filter(ready_report)
}

/// Idle/query path for `kiss test --lang`: answer from a ready unscoped workspace
/// report by projecting that language's rows, without starting a new test cycle.
///
/// `load_ready_for_request` stays identity-strict (no parent slice on load alone).
pub(crate) fn project_language_ready_from_parent_workspace(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let lang = request.language()?;
    if !super::is_workspace_focus(&request.focus) {
        return None;
    }
    let mut parent_req = request.clone();
    parent_req.set_language(None);
    let parent = load_ready_for_request(repo, &parent_req, extras)?;
    let label = lang.label();
    let rows: Vec<_> = parent
        .rows
        .iter()
        .filter(|row| row.language == label)
        .cloned()
        .collect();
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let (projection, _) = super::projection::build_slice_projection(repo, request, &resolved);
    let mut stamp = super::slice::stamp_from_projection(&projection, parent.stamp.complete);
    stamp.complete = parent.stamp.complete;
    if !stamp.complete {
        return None;
    }
    let selectors: Vec<String> = rows.iter().map(|row| row.selector.clone()).collect();
    let scope = super::scope::ReportScope::from_membership(
        projection.report_regions(),
        selectors,
        stamp.complete,
    );
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut built = TargetReport::assembled_in(repo, request, scope, rows, stamp, exit_code);
    built.snapshot.extras = parent.snapshot.extras.clone();
    built.snapshot.worktree = super::stamp::capture_worktree_token(repo, Some(lang));
    Some(built)
}

/// Answer `commit` / `base` / `main` from the cached workspace results.
/// A deleted test file adds no rows. A gitignored path is out of scope.
pub(crate) fn project_git_ready_from_parent_workspace(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let super::types::TargetFocus::Git(_) = &request.focus else {
        return None;
    };
    let mut parent_req = request.clone();
    parent_req.focus = super::types::TargetFocus::Workspace;
    parent_req.set_language(None);
    let parent = load_ready_for_request(repo, &parent_req, extras)?;
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let selectors = git_answer_selectors(repo, request, &resolved);
    let rows = git_rows_for_selectors(&parent, &selectors);
    let (projection, _) = super::projection::build_slice_projection(repo, request, &resolved);
    let mut stamp = super::slice::stamp_from_projection(&projection, true);
    stamp.complete = true;
    let scope =
        super::scope::ReportScope::from_membership(projection.report_regions(), selectors, true);
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut built = TargetReport::assembled_in(repo, request, scope, rows, stamp, exit_code);
    built.snapshot.extras = parent.snapshot.extras.clone();
    built.snapshot.worktree = super::stamp::capture_worktree_token(repo, request.language());
    Some(built)
}

fn git_answer_selectors(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    resolved: &super::resolved::ResolvedTarget,
) -> Vec<String> {
    let changed = git_changed_existing(resolved);
    let deleted: std::collections::BTreeSet<String> =
        resolved.historical_paths.iter().cloned().collect();
    let gitignore = crate::test_runner::workspace_selector_cache::support_gitignore(repo);
    let mut universe = Vec::new();
    if let Some(python) =
        crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors(
            repo,
            &request.ignore,
            &[],
        )
    {
        universe.extend(python);
    }
    if let Some(rust) =
        crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
            repo,
            &request.ignore,
        )
    {
        universe.extend(rust);
    }
    universe.sort();
    universe.dedup();
    let changed_paths: Vec<String> = changed.iter().cloned().collect();
    let population: std::collections::BTreeSet<String> =
        super::projection::population_selectors_for_paths(repo, &changed_paths, request)
            .into_iter()
            .collect();
    let mut selected = Vec::new();
    for selector in universe {
        let file = selector
            .split_once("::")
            .map_or(selector.as_str(), |(file, _)| file);
        if deleted.contains(file) || path_ignored(&gitignore, file, &request.ignore) {
            continue;
        }
        if changed.contains(file) || population.contains(&selector) {
            selected.push(selector);
        }
    }
    if let Some(lang) = request.language() {
        let needle = format!(".{}", lang.extension());
        selected.retain(|selector| selector.contains(&needle));
    }
    selected.sort();
    selected.dedup();
    selected
}

fn git_changed_existing(
    resolved: &super::resolved::ResolvedTarget,
) -> std::collections::BTreeSet<String> {
    resolved
        .regions
        .iter()
        .filter_map(|region| match region {
            super::resolved::SourceRegion::FileAll { path }
            | super::resolved::SourceRegion::FileLines { path, .. } => Some(path.clone()),
            super::resolved::SourceRegion::WorkspaceAll => None,
        })
        .collect()
}

fn path_ignored(gitignore: &ignore::gitignore::Gitignore, rel: &str, prefixes: &[String]) -> bool {
    gitignore.matched(rel, false).is_ignore() || kiss::path_ignored_by_prefixes(rel, prefixes)
}

fn git_rows_for_selectors(
    parent: &TargetReport,
    selectors: &[String],
) -> Vec<super::report::SelectorRow> {
    selectors
        .iter()
        .filter_map(|selector| parent.rows.iter().find(|row| row.selector == *selector))
        .cloned()
        .collect()
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
        for language in kiss::Language::ALL
            .into_iter()
            .filter(|language| language.allowed_by(args.target_request.language()))
        {
            crate::test_runner::lang_registry::rules_for(language)
                .validate_extra_args(args.extras.get(language))?;
        }
        match preview_target_plan_with(&repo, &request, &EnsurePolicy::preview(args.force_bad)) {
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
        &EnsurePolicy::complete(args.force_bad),
        Some(args),
    ) {
        Ok(Ensured::Report(report)) => {
            let executed = super::counters::current().subprocess > 0;
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
