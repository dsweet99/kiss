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
    .filter(|report| {
        report.snapshot.worktree
            == super::stamp::capture_worktree_token(repo, request.language())
    })
    .filter(|report| report.snapshot.extras.as_slices() == extras)
    .filter(|report| super::report::pinned_graph_generation_holds(repo, coverage_all, report))
}

/// Idle/query path for `kiss test --lang`: answer from a ready unscoped workspace
/// report by projecting that language's rows, without starting a new test cycle.
///
/// `load_ready_for_request` stays identity-strict (no parent slice on load alone).
pub(crate) fn project_language_ready_from_parent_workspace(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let lang = request.language()?;
    if !super::is_workspace_focus(&request.focus) {
        return None;
    }
    let mut parent_req = request.clone();
    parent_req.set_language(None);
    let parent = load_ready_for_request(repo, &parent_req, coverage_all, extras)?;
    let label = match lang {
        kiss::Language::Python => "python",
        kiss::Language::Rust => "rust",
    };
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
        projection.coverage_regions(),
        selectors,
        stamp.complete,
    );
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut built = TargetReport::assembled_in(
        repo,
        request,
        scope,
        rows,
        stamp,
        exit_code,
        coverage_all,
    );
    built.snapshot.extras = parent.snapshot.extras.clone();
    built.snapshot.worktree = super::stamp::capture_worktree_token(repo, Some(lang));
    let _ = report_store::publish_report(repo, request, &built);
    Some(built)
}

/// Answer an operand query from a ready workspace report. The client does not
/// start a cycle; rows outside the operand are omitted.
pub(crate) fn project_operand_ready_from_parent_workspace(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let super::types::TargetFocus::Operands(operands) = &request.focus else {
        return None;
    };
    let mut parent_req = request.clone();
    parent_req.focus = super::types::TargetFocus::Workspace;
    parent_req.set_language(None);
    let parent = load_ready_for_request(repo, &parent_req, coverage_all, extras)
        .or_else(|| report_store::load_current_report(repo))?;
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let expected = operand_expected_selectors(repo, &resolved);
    let lang = request.language().map(|language| match language {
        kiss::Language::Python => "python",
        kiss::Language::Rust => "rust",
    });
    let rows: Vec<_> = parent
        .rows
        .iter()
        .filter(|row| lang.is_none_or(|label| row.language == label))
        .filter(|row| selector_in_operand_scope(&row.selector, &expected, operands))
        .cloned()
        .collect();
    let (projection, _) = super::projection::build_slice_projection(repo, request, &resolved);
    let mut stamp = super::slice::stamp_from_projection(&projection, parent.stamp.complete);
    stamp.complete = parent.stamp.complete;
    if !stamp.complete {
        return None;
    }
    let selectors: Vec<String> = rows.iter().map(|row| row.selector.clone()).collect();
    let scope = super::scope::ReportScope::from_membership(
        projection.coverage_regions(),
        selectors,
        stamp.complete,
    );
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut built = TargetReport::assembled_in(
        repo,
        request,
        scope,
        rows,
        stamp,
        exit_code,
        coverage_all,
    );
    built.snapshot.extras = parent.snapshot.extras.clone();
    built.snapshot.worktree =
        super::stamp::capture_worktree_token(repo, request.language());
    let _ = report_store::publish_report(repo, request, &built);
    Some(built)
}

fn operand_expected_selectors(
    repo: &std::path::Path,
    resolved: &super::resolved::ResolvedTarget,
) -> Vec<String> {
    let mut expected = resolved.direct_selectors.clone();
    let paths: Vec<String> = resolved
        .regions
        .iter()
        .filter_map(|region| match region {
            super::resolved::SourceRegion::FileAll { path }
            | super::resolved::SourceRegion::FileLines { path, .. } => Some(path.clone()),
            super::resolved::SourceRegion::WorkspaceAll => None,
        })
        .collect();
    expected.extend(
        super::history::reverse_records(repo, &paths)
            .into_iter()
            .flat_map(|record| record.selectors),
    );
    expected.sort();
    expected.dedup();
    expected
}

fn selector_in_operand_scope(
    selector: &str,
    expected: &[String],
    operands: &[super::types::OperandExpr],
) -> bool {
    let by_expected = expected.iter().any(|item| {
        selector == item || selector.starts_with(&format!("{item}::"))
    });
    if by_expected {
        return true;
    }
    operands.iter().any(|operand| {
        let path = operand.raw.split_once("::").map_or(operand.raw.as_str(), |(path, _)| path);
        selector == operand.raw
            || selector.starts_with(&format!("{}::", operand.raw))
            || (!path.is_empty() && (selector == path || selector.starts_with(&format!("{path}::")) || selector.starts_with(&format!("{path}/"))))
    })
}

/// Answer `commit` / `base` / `main` from the cached workspace results.
/// A deleted test file adds no rows. A gitignored path is out of scope.
pub(crate) fn project_git_ready_from_parent_workspace(
    repo: &std::path::Path,
    request: &super::types::TargetRequest,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let super::types::TargetFocus::Git(_) = &request.focus else {
        return None;
    };
    let mut parent_req = request.clone();
    parent_req.focus = super::types::TargetFocus::Workspace;
    parent_req.set_language(None);
    let parent = load_ready_for_request(repo, &parent_req, coverage_all, extras)
        .or_else(|| report_store::load_current_report(repo))?;
    let resolved = super::resolve::resolve_only(repo, request).ok()?;
    let selectors = git_answer_selectors(repo, request, &resolved);
    let rows = git_rows_for_selectors(repo, &parent, &selectors);
    let (projection, _) = super::projection::build_slice_projection(repo, request, &resolved);
    let mut stamp = super::slice::stamp_from_projection(&projection, true);
    stamp.complete = true;
    let scope = super::scope::ReportScope::from_membership(
        projection.coverage_regions(),
        selectors,
        true,
    );
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut built = TargetReport::assembled_in(
        repo,
        request,
        scope,
        rows,
        stamp,
        exit_code,
        coverage_all,
    );
    built.snapshot.extras = parent.snapshot.extras.clone();
    built.snapshot.worktree = super::stamp::capture_worktree_token(repo, request.language());
    let _ = report_store::publish_report(repo, request, &built);
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
    let gitignore = crate::test_runner::workspace_selector_cache::watch_support_gitignore(repo);
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
    let outcomes = python_cached_outcomes(repo, &universe);
    let mut selected = Vec::new();
    for selector in universe {
        let file = selector.split_once("::").map_or(selector.as_str(), |(file, _)| file);
        if deleted.contains(file) || path_ignored(&gitignore, file, &request.ignore) {
            continue;
        }
        let defined_in_change = changed.contains(file);
        let covers_change = outcomes
            .get(&selector)
            .is_some_and(|outcome| outcome_covers(repo, outcome, &changed));
        if defined_in_change || covers_change {
            selected.push(selector);
        }
    }
    if let Some(lang) = request.language() {
        let needle = match lang {
            kiss::Language::Python => ".py",
            kiss::Language::Rust => ".rs",
        };
        selected.retain(|selector| selector.contains(needle));
    }
    selected.sort();
    selected.dedup();
    selected
}

fn git_changed_existing(resolved: &super::resolved::ResolvedTarget) -> std::collections::BTreeSet<String> {
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
    gitignore
        .matched(rel, false)
        .is_ignore()
        || kiss::path_ignored_by_prefixes(rel, prefixes)
}

fn python_cached_outcomes(
    repo: &std::path::Path,
    selectors: &[String],
) -> std::collections::BTreeMap<String, kiss::rslip::RslipOutcome> {
    let Ok((python_version, pytest_version)) =
        crate::test_runner::lang_python::rslip_request::detect_rslip_versions(repo)
    else {
        return std::collections::BTreeMap::new();
    };
    let gate = kiss::GateConfig::load_for_repo(repo);
    let python: Vec<&String> = selectors.iter().filter(|selector| selector.contains(".py")).collect();
    let reqs: Vec<_> = python
        .iter()
        .filter_map(|selector| {
            crate::test_runner::lang_python::rslip_request::rslip_request_from_parts(
                repo,
                selector,
                &[],
                &python_version,
                &pytest_version,
                false,
                &gate,
            )
            .ok()
        })
        .collect();
    let loaded = kiss::rslip::load_cached_outcomes_many_trusting_population(&reqs);
    let mut out = std::collections::BTreeMap::new();
    for (selector, outcome) in python.into_iter().zip(loaded) {
        if let Ok(Some(outcome)) = outcome {
            out.insert(selector.clone(), outcome);
        }
    }
    out
}

fn outcome_covers(
    repo: &std::path::Path,
    outcome: &kiss::rslip::RslipOutcome,
    changed: &std::collections::BTreeSet<String>,
) -> bool {
    outcome.coverage.files.keys().any(|recorded| {
        recorded_rel(repo, recorded).is_some_and(|rel| changed.contains(&rel))
    })
}

fn recorded_rel(repo: &std::path::Path, recorded: &str) -> Option<String> {
    let path = std::path::Path::new(recorded);
    let rel = path.strip_prefix(repo).ok().or_else(|| {
        let canon = repo.canonicalize().ok()?;
        path.strip_prefix(canon).ok()
    });
    rel.map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .or_else(|| {
            let spelled = recorded.trim_start_matches("./").replace('\\', "/");
            (!spelled.starts_with('/')).then_some(spelled)
        })
}

fn git_rows_for_selectors(
    repo: &std::path::Path,
    parent: &TargetReport,
    selectors: &[String],
) -> Vec<super::report::SelectorRow> {
    let outcomes = python_cached_outcomes(repo, selectors);
    selectors
        .iter()
        .filter_map(|selector| {
            if let Some(row) = parent.rows.iter().find(|row| row.selector == *selector) {
                return Some(row.clone());
            }
            let outcome = outcomes.get(selector)?;
            let effective = match outcome.status {
                kiss::rpytest_runner::TestStatus::Passed => super::report::EffectiveStatus::Pass,
                kiss::rpytest_runner::TestStatus::Failed => super::report::EffectiveStatus::Fail,
                kiss::rpytest_runner::TestStatus::TimedOut => super::report::EffectiveStatus::Timeout,
            };
            let language = if selector.contains(".py") {
                "python"
            } else {
                "rust"
            };
            Some(super::report::SelectorRow {
                language: language.to_string(),
                selector: selector.clone(),
                raw: match outcome.status {
                    kiss::rpytest_runner::TestStatus::Passed => "passed",
                    kiss::rpytest_runner::TestStatus::Failed => "failed",
                    kiss::rpytest_runner::TestStatus::TimedOut => "timed_out",
                }
                .to_string(),
                effective,
                duration_ns: Some(u64::try_from(outcome.duration.as_nanos()).unwrap_or(u64::MAX)),
                provenance: "cache".into(),
            })
        })
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
