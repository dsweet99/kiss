use std::path::Path;
use std::time::Instant;

use super::projection::{build_slice_projection, remember_target_plan, slice_for};
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

/// Snapshot / ensure behavior as named modes — not a free bool bag.
/// Invalid combinations such as `dry_run` with `require_complete` are not constructible
/// through the production constructors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EnsurePolicy {
    dry_run: bool,
    require_complete: bool,
    inject_mismatch: bool,
    retry_bad: bool,
    coverage_all: bool,
    assemble_only: bool,
}

impl EnsurePolicy {
    /// Dry-run plan preview (`bind` / `preview_target_plan_with`).
    pub(crate) fn preview(retry_bad: bool, coverage_all: bool) -> Self {
        Self {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad,
            coverage_all,
            assemble_only: false,
        }
    }

    /// Full ensure that requires complete membership evidence.
    pub(crate) fn complete(retry_bad: bool, coverage_all: bool) -> Self {
        Self {
            dry_run: false,
            require_complete: true,
            inject_mismatch: false,
            retry_bad,
            coverage_all,
            assemble_only: false,
        }
    }

    /// Idle / report query: complete membership, no retry-bad, no assembly repair.
    pub(crate) fn query(coverage_all: bool) -> Self {
        Self::complete(false, coverage_all)
    }

    /// Assemble a report from witnesses without running repair subprocesses.
    #[cfg(test)]
    pub(crate) fn assemble(coverage_all: bool) -> Self {
        Self {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all,
            assemble_only: true,
        }
    }

    /// Soft materialize: may omit complete-membership errors (tests / tolerant paths).
    #[cfg(test)]
    pub(crate) fn soft(retry_bad: bool, coverage_all: bool) -> Self {
        Self {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad,
            coverage_all,
            assemble_only: false,
        }
    }

    /// Test-only fault injection for concurrent-mutation retry paths.
    #[cfg(test)]
    pub(crate) fn inject_mismatch_for_test() -> Self {
        Self {
            dry_run: false,
            require_complete: false,
            inject_mismatch: true,
            retry_bad: false,
            coverage_all: false,
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

    pub(crate) fn coverage_all(&self) -> bool {
        self.coverage_all
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
    remember_target_plan(repo_root, request, &resolved_again);
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors.clone());
    let mut scope =
        ReportScope::from_membership(projection.coverage_regions(), selectors, stamp.complete);
    apply_runner_extra(repo_root, &mut scope, args)?;
    let available = super::rows::available_rows(repo_root, &scope);
    let graph_repair = super::report::graph_repair_needed(repo_root, &scope, policy.coverage_all());
    let force = args.is_some_and(|item| item.force_rerun);
    let time_gate_active = time_gate_active(repo_root, args);
    let plan = super::rows::plan_from_available_rows_with(
        &scope,
        &available,
        policy.retry_bad(),
        graph_repair,
        force,
        time_gate_active,
    );
    if policy.dry_run() {
        let _ = plan.known_execution_union();
        let deferred = !scope.complete || (policy.retry_bad() && !plan.repair_selectors.is_empty());
        return Ok(SnapshotOutcome::Preview(TargetPlanPreview {
            deferred,
            membership_complete: scope.complete,
            scope,
            plan,
        }));
    }
    let deferred_edit = crate::test_runner::ensure_runtime::deferred_edit_pending();
    if !policy.retry_bad() && !force && !deferred_edit {
        let extras = args
            .map(|item| item.extras)
            .unwrap_or(crate::test_runner::language_keyed::LanguageKeyed::EMPTY);
        if let Some(ready) =
            super::bind::load_ready_for_request(repo_root, request, policy.coverage_all(), extras)
        {
            return Ok(SnapshotOutcome::Report(Box::new(ready)));
        }
    }
    if graph_repair {
        super::report::repair_graph_evidence(repo_root, &scope, policy.coverage_all())
            .map_err(EnsureError::IncompleteEvidence)?;
    }
    if !policy.assemble_only()
        && let Some(args) = args
        && (!plan.known_execution_union().is_empty()
            || plan.population_repair
            || deferred_edit)
    {
        execute_repair(args)?;
        return assemble_after_repair(repo_root, request, policy, time_gate_active, args);
    }
    if args.is_some_and(|item| !item.extras.rust.is_empty())
        && scope.selectors.is_empty()
        && policy.require_complete()
    {
        return Err(EnsureError::IncompleteEvidence(
            crate::test_runner::runners::NO_COVERING_TESTS_MSG.into(),
        ));
    }
    if policy.require_complete() && !stamp.complete {
        return Err(EnsureError::IncompleteEvidence(
            "target membership is not proven complete".into(),
        ));
    }
    // No-repair assemble (e.g. post-SIGINT restart with rslip hits only) still needs
    // workspace selector evidence for max_num_tests. assemble_after_repair refreshes;
    // this path must too or gates_from_population fails closed as incomplete.
    if let Some(args) = args {
        refresh_python_witnesses(repo_root, &scope, args)
            .map_err(EnsureError::IncompleteEvidence)?;
    }
    assemble_report(
        repo_root,
        request,
        policy,
        scope,
        stamp,
        time_gate_active,
        args,
    )
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
    time_gate_active: bool,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Result<SnapshotOutcome, EnsureError> {
    let prelim = resolve_only(repo_root, request).map_err(EnsureError::Planning)?;
    let (prelim_projection, prelim_complete) = build_slice_projection(repo_root, request, &prelim);
    let mut prelim_selectors = prelim_projection.selectors();
    prelim_selectors.extend(prelim.direct_selectors);
    let prelim_scope = ReportScope::from_membership(
        prelim_projection.coverage_regions(),
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
    remember_target_plan(repo_root, request, &resolved_again);
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors);
    let mut scope =
        ReportScope::from_membership(projection.coverage_regions(), selectors, stamp.complete);
    apply_runner_extra(repo_root, &mut scope, Some(args))?;
    if !args.extras.rust.is_empty() && scope.selectors.is_empty() && policy.require_complete() {
        return Err(EnsureError::IncompleteEvidence(
            crate::test_runner::runners::NO_COVERING_TESTS_MSG.into(),
        ));
    }
    assemble_report(
        repo_root,
        request,
        policy,
        scope,
        stamp,
        time_gate_active,
        Some(args),
    )
}

fn refresh_python_witnesses(
    repo_root: &Path,
    scope: &ReportScope,
    args: &crate::test_runner::RunTestCmdArgs<'_>,
) -> Result<(), String> {
    let python: Vec<String> = scope
        .selectors
        .iter()
        .filter(|selector| selector.contains(".py"))
        .cloned()
        .collect();
    if let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(repo_root)
        && python_scope_has_typed_rows(&pinned, &python)
        && crate::test_runner::lang_python::generation::identity_matches_current(
            repo_root,
            &pinned.plan.base_identity,
            args.extras.python,
        )
    {
        store_live_python_selectors(repo_root, args.ignore(), &pinned.plan.selectors);
        return Ok(());
    }
    let discovered = crate::test_runner::lang_python::collect::collect_python_nodeids(
        repo_root,
        None,
        args.extras.python,
    )
    .unwrap_or_else(|_| python.clone());
    if !discovered.is_empty() {
        crate::test_runner::python_coverage_index::publish_python_generation_from_cached_selectors(
            repo_root,
            &discovered,
            args.extras.python,
            &args.gate_config,
        )?;
    }
    let stored = if discovered.is_empty() {
        Vec::new()
    } else {
        live_python_selectors(repo_root, &discovered)
    };
    store_live_python_selectors(repo_root, args.ignore(), &stored);
    Ok(())
}

fn store_live_python_selectors(repo_root: &Path, ignore: &[String], selectors: &[String]) {
    let live = live_python_selectors(repo_root, selectors);
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

fn python_scope_has_typed_rows(
    pinned: &crate::test_runner::python_coverage_index::generation::PinnedPythonGeneration,
    python: &[String],
) -> bool {
    if python.is_empty() {
        return false;
    }
    let witness = crate::test_runner::lang_python::python_witness_from_pinned(pinned);
    python.iter().all(|selector| {
        witness
            .selectors
            .iter()
            .position(|item| item == selector)
            .and_then(|idx| witness.raw_statuses.get(idx).copied())
            .is_some_and(|status| {
                status != crate::test_runner::lang_iface::WitnessStatus::Unresolved
            })
    })
}

fn apply_runner_extra(
    repo_root: &Path,
    scope: &mut ReportScope,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<(), EnsureError> {
    let Some(args) = args else {
        return Ok(());
    };
    if args.extras.rust.is_empty() {
        return Ok(());
    }
    let discovered = crate::test_runner::lang_python::collect::collect_python_nodeids(
        repo_root,
        None,
        args.extras.python,
    )
    .unwrap_or_default();
    scope.selectors.retain(|selector| {
        !selector.contains(".py") || discovered.iter().any(|item| item == selector)
    });
    Ok(())
}

fn assemble_report(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    scope: ReportScope,
    stamp: super::slice::TargetSliceStamp,
    time_gate_active: bool,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
) -> Result<SnapshotOutcome, EnsureError> {
    let rows = super::rows::rows_from_witnesses(repo_root, &scope)
        .map_err(EnsureError::IncompleteEvidence)?;
    super::rows::duration_evidence_holds(&rows, time_gate_active)
        .map_err(EnsureError::IncompleteEvidence)?;
    let exit_code = TargetReport::exit_from_rows(&rows);
    let mut report = TargetReport::assembled_in(
        repo_root,
        request,
        scope,
        rows,
        stamp,
        exit_code,
        policy.coverage_all(),
    );
    if let Some(args) = args {
        report.snapshot.extras = args.extras.owned_vecs();
    }
    Ok(SnapshotOutcome::Report(Box::new(report)))
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
