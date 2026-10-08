use std::path::Path;
use std::time::Instant;

use super::projection::{build_slice_projection, slice_for};
use super::report::{TargetPlanPreview, TargetReport};
use super::resolve::resolve_only;
use super::scope::ReportScope;
use super::slice::stamp_from_projection;
use super::types::TargetRequest;

pub(crate) const MAX_SNAPSHOT_ATTEMPTS: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EnsureError {
    IncompleteEvidence(String),
    ConcurrentMutation,
    Planning(String),
    Interrupted,
}

impl EnsureError {
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            Self::Interrupted => 130,
            _ => 1,
        }
    }
}

impl std::fmt::Display for EnsureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompleteEvidence(msg) | Self::Planning(msg) => write!(f, "{msg}"),
            Self::ConcurrentMutation => write!(f, "concurrent mutation"),
            Self::Interrupted => write!(f, "interrupted"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EnsurePolicy {
    dry_run: bool,
    require_complete: bool,
    inject_mismatch: bool,
    retry_bad: bool,
    assemble_only: bool,
}

impl EnsurePolicy {
    pub(crate) fn preview(retry_bad: bool) -> Self {
        Self {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad,
            assemble_only: false,
        }
    }

    pub(crate) fn complete(retry_bad: bool) -> Self {
        Self {
            dry_run: false,
            require_complete: true,
            inject_mismatch: false,
            retry_bad,
            assemble_only: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn soft(retry_bad: bool) -> Self {
        Self {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad,
            assemble_only: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_mismatch_for_test() -> Self {
        Self {
            dry_run: false,
            require_complete: false,
            inject_mismatch: true,
            retry_bad: false,
            assemble_only: false,
        }
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn require_complete(&self) -> bool {
        self.require_complete
    }

    pub(crate) fn inject_mismatch(&self) -> bool {
        self.inject_mismatch
    }

    pub(crate) fn retry_bad(&self) -> bool {
        self.retry_bad
    }

    pub(crate) fn assemble_only(&self) -> bool {
        self.assemble_only
    }
}

pub(crate) fn run_snapshot_kernel(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
) -> Result<SnapshotOutcome, EnsureError> {
    run_snapshot_kernel_with(repo_root, request, policy, None)
}

pub(crate) fn run_snapshot_kernel_with(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<SnapshotOutcome, EnsureError> {
    let mut attempts = 0;
    while attempts < MAX_SNAPSHOT_ATTEMPTS {
        attempts += 1;
        match try_snapshot(repo_root, request, policy, args) {
            Ok(outcome) => return Ok(outcome),
            Err(EnsureError::ConcurrentMutation) if attempts < MAX_SNAPSHOT_ATTEMPTS => {
                super::counters::add_snapshot_retry();
            }
            Err(err) => return Err(err),
        }
    }
    Err(EnsureError::ConcurrentMutation)
}

pub(crate) enum SnapshotOutcome {
    Report(Box<TargetReport>),
    Preview(TargetPlanPreview),
}

fn try_snapshot(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<SnapshotOutcome, EnsureError> {
    let resolved = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let (projection, complete) = build_slice_projection(repo_root, request, &resolved);
    let stamp = stamp_from_projection(&projection, complete);
    if policy.inject_mismatch() {
        return Err(EnsureError::ConcurrentMutation);
    }
    let resolved_again = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let stamp_again = slice_for(repo_root, request, &resolved_again);
    if stamp != stamp_again {
        return Err(EnsureError::ConcurrentMutation);
    }
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors.clone());
    let mut scope =
        ReportScope::from_membership(projection.report_regions(), selectors, stamp.complete);
    apply_runner_extra(repo_root, &mut scope, args)?;
    let available = super::rows::available_rows(repo_root, &scope, runner_extras(args));
    let graph_repair = super::report::graph_repair_needed(repo_root, &scope);
    let force = args.is_some_and(|item| item.force_rerun);
    let row_plan = super::rows::AvailableRowPlan {
        retry_bad: policy.retry_bad(),
        graph_repair,
        force,
        time_gate_active: time_gate_active(repo_root, args),
    };
    let plan = super::rows::plan_from_available_rows(&scope, &available, row_plan);
    snapshot_finish::finish_planned_snapshot(
        repo_root,
        request,
        policy,
        args,
        snapshot_finish::PlannedSnapshot {
            stamp,
            scope,
            plan,
            row_plan,
        },
    )
}

#[path = "snapshot_finish.rs"]
mod snapshot_finish;

#[derive(Clone, Copy)]
struct RunFacts {
    time_gate_active: bool,
    runner_exit: i32,
}

fn execute_repair(args: &crate::test_runner::RunTestCmdArgs<'_>) -> Result<i32, EnsureError> {
    super::counters::add_subprocess();
    match crate::test_runner::run_live_overlapped_test(args, Instant::now()) {
        Ok(code) => Ok(code),
        Err(err) => {
            if crate::test_runner::consume_rust_batch_interrupted() {
                return Err(EnsureError::Interrupted);
            }
            Err(EnsureError::Planning(err))
        }
    }
}

fn assemble_after_repair(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    facts: RunFacts,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Result<SnapshotOutcome, EnsureError> {
    let prelim = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let (prelim_projection, prelim_complete) = build_slice_projection(repo_root, request, &prelim);
    let mut prelim_selectors = prelim_projection.selectors();
    prelim_selectors.extend(prelim.direct_selectors);
    let prelim_scope = ReportScope::from_membership(
        prelim_projection.report_regions(),
        prelim_selectors,
        prelim_complete,
    );
    refresh_python_witnesses(repo_root, &prelim_scope, args)
        .map_err(EnsureError::IncompleteEvidence)?;
    let resolved = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let (projection, complete) = build_slice_projection(repo_root, request, &resolved);
    let stamp = stamp_from_projection(&projection, complete);
    let resolved_again = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let stamp_again = slice_for(repo_root, request, &resolved_again);
    if stamp != stamp_again {
        return Err(EnsureError::ConcurrentMutation);
    }
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors);
    let mut scope =
        ReportScope::from_membership(projection.report_regions(), selectors, stamp.complete);
    apply_runner_extra(repo_root, &mut scope, Some(args))?;
    if extras_select_tests(args) && scope.selectors.is_empty() && policy.require_complete() {
        return Err(EnsureError::IncompleteEvidence(
            crate::test_runner::runners::NO_SELECTED_TESTS_MSG.into(),
        ));
    }
    assemble_report(repo_root, request, scope, stamp, facts, Some(args))
}

fn refresh_python_witnesses(
    repo_root: &Path,
    scope: &ReportScope,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Result<(), String> {
    let python: Vec<String> = scope
        .selectors
        .iter()
        .filter(|selector| selector_is(selector, kiss::Language::Python))
        .cloned()
        .collect();
    let discovered = crate::test_runner::runners::enumerate_workspace_python_selectors(
        repo_root,
        args.ignore(),
        args.extras.python,
    )
    .unwrap_or_else(|_| python.clone());
    let stored = if discovered.is_empty() {
        Vec::new()
    } else {
        live_python_selectors(repo_root, &discovered)
    };
    store_live_python_selectors(repo_root, args.ignore(), &stored);
    Ok(())
}

fn store_live_python_selectors(repo_root: &Path, ignore: &[String], selectors: &[String]) {
    let live: Vec<String> = live_python_selectors(repo_root, selectors)
        .into_iter()
        .filter(|selector| !kiss::selector_ignored_by_prefixes(selector, ignore))
        .collect();
    let _ = crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
        repo_root,
        ignore,
        &live,
        &[],
    );
}

fn live_python_selectors(repo_root: &Path, selectors: &[String]) -> Vec<String> {
    selectors
        .iter()
        .filter(|selector| {
            let path = selector
                .split_once("::")
                .map(|(path, _)| path)
                .unwrap_or(selector);
            repo_root.join(path).is_file()
        })
        .cloned()
        .collect()
}

fn apply_runner_extra(
    repo_root: &Path,
    scope: &mut ReportScope,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<(), EnsureError> {
    let Some(args) = args else {
        return Ok(());
    };
    let python_extras = args.extras.get(kiss::Language::Python);
    if python_extras.is_empty() {
        return Ok(());
    }
    let discovered = crate::test_runner::lang_python::collect::collect_python_nodeids(
        repo_root,
        None,
        python_extras,
    )
    .unwrap_or_default();
    scope.selectors.retain(|selector| {
        !selector_is(selector, kiss::Language::Python)
            || discovered.iter().any(|item| item == selector)
    });
    Ok(())
}

pub(super) fn extras_select_tests(args: &crate::test_runner::RunTestCmdArgs<'_>) -> bool {
    kiss::Language::ALL
        .into_iter()
        .any(|language| !args.extras.get(language).is_empty())
}

fn selector_is(selector: &str, language: kiss::Language) -> bool {
    let path = selector
        .split_once("::")
        .map(|(path, _)| path)
        .unwrap_or(selector);
    kiss::Language::from_path(Path::new(path)) == Some(language)
}

fn assemble_report(
    repo_root: &Path,
    request: &TargetRequest,
    scope: ReportScope,
    stamp: super::slice::TargetSliceStamp,
    facts: RunFacts,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<SnapshotOutcome, EnsureError> {
    let rows = super::rows::rows_from_witnesses(repo_root, &scope, runner_extras(args))
        .map_err(EnsureError::IncompleteEvidence)?;
    super::rows::duration_evidence_holds(&rows, facts.time_gate_active)
        .map_err(EnsureError::IncompleteEvidence)?;
    let exit_code = assembled_exit(&rows, facts.runner_exit);
    let mut report = TargetReport::assembled_in(repo_root, request, scope, rows, stamp, exit_code);
    if let Some(args) = args {
        report.snapshot.extras = args.extras.owned_vecs();
    }
    Ok(SnapshotOutcome::Report(Box::new(report)))
}

pub(crate) fn assembled_exit(rows: &[super::report::SelectorRow], runner_exit: i32) -> i32 {
    TargetReport::combine_exit(TargetReport::exit_from_rows(rows), runner_exit)
}

fn time_gate_active(
    repo_root: &Path,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> bool {
    let gate = args
        .map(|item| item.gate_config.clone())
        .unwrap_or_else(|| kiss::GateConfig::load_for_repo(repo_root));
    !gate.max_unit_test_seconds.is_empty()
}

fn runner_extras<'a>(
    args: Option<&crate::test_runner::RunTestCmdArgs<'a>>,
) -> crate::test_runner::language_keyed::LanguageKeyed<&'a [String]> {
    args.map_or(
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        |item| item.extras,
    )
}

#[cfg(test)]
mod selector_language_tests {
    use super::selector_is;

    #[test]
    fn selector_membership_uses_the_language_enum() {
        assert!(selector_is("tests/a.py::test_a", kiss::Language::Python));
        assert!(selector_is("src/lib.rs::case", kiss::Language::Rust));
        assert!(!selector_is("notes.py.bak::test_a", kiss::Language::Python));
        assert!(!selector_is(
            "crate::mod::test_py_name",
            kiss::Language::Python
        ));
    }
}
