#![cfg_attr(not(test), allow(dead_code))]
use std::path::Path;

use super::{
    EnsureError, EnsurePolicy, ReportScope, RunFacts, SnapshotOutcome, TargetPlanPreview,
    TargetRequest, assemble_after_repair, assemble_report, execute_repair,
    refresh_python_witnesses,
};

pub(super) struct PlannedSnapshot {
    pub(super) stamp: super::super::slice::TargetSliceStamp,
    pub(super) scope: ReportScope,
    pub(super) plan: super::super::scope::ExecutionPlan,
    pub(super) row_plan: super::super::rows::AvailableRowPlan,
}

pub(super) fn finish_planned_snapshot(
    repo_root: &Path,
    request: &TargetRequest,
    policy: &EnsurePolicy,
    args: Option<&crate::test_runner::RunTestCmdArgs<'_>>,
    planned: PlannedSnapshot,
) -> Result<SnapshotOutcome, EnsureError> {
    let PlannedSnapshot {
        stamp,
        scope,
        plan,
        row_plan,
    } = planned;
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
    if row_plan.graph_repair {
        super::super::report::repair_graph_evidence(repo_root, &scope)
            .map_err(EnsureError::IncompleteEvidence)?;
    }
    let target_selected = !scope.selectors.is_empty();
    if !policy.assemble_only()
        && let Some(args) = args
        && (target_selected || !plan.known_execution_union().is_empty() || plan.population_repair)
    {
        let facts = RunFacts {
            time_gate_active: row_plan.time_gate_active,
            runner_exit: execute_repair(args)?,
        };
        return assemble_after_repair(repo_root, request, policy, facts, args);
    }
    if args.is_some_and(super::extras_select_tests)
        && scope.selectors.is_empty()
        && policy.require_complete()
    {
        return Err(EnsureError::IncompleteEvidence(
            crate::test_runner::runners::NO_SELECTED_TESTS_MSG.into(),
        ));
    }
    if policy.require_complete() && !stamp.complete {
        return Err(EnsureError::IncompleteEvidence(
            "target membership is not proven complete".into(),
        ));
    }
    if let Some(args) = args {
        refresh_python_witnesses(repo_root, &scope, args)
            .map_err(EnsureError::IncompleteEvidence)?;
    }
    assemble_report(
        repo_root,
        request,
        scope,
        stamp,
        RunFacts {
            time_gate_active: row_plan.time_gate_active,
            runner_exit: 0,
        },
        args,
    )
}
