use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use std::fs;

use super::canon::canonicalize_target_request;
use super::ensure::{
    EnsureOutcome, assemble_target_report_query, ensure_target_report, ensure_target_report_query,
    materialize_target_report, preview_target_plan_with,
};
use super::snapshot::{EnsureError, EnsurePolicy, MAX_SNAPSHOT_ATTEMPTS};
use super::types::{OperandExpr, TargetFocus, TargetRequest};

#[test]
fn ensure_policy_named_modes_exclude_invalid_dry_run_require_complete() {
    // PWS: free bool bags allowed dry_run∧require_complete; named modes must not.
    let query = EnsurePolicy::query(false);
    let preview = EnsurePolicy::preview(true, true);
    let complete = EnsurePolicy::complete(true, false);
    assert!(!query.dry_run() && query.require_complete());
    assert!(!query.retry_bad() && !query.inject_mismatch() && !query.assemble_only());
    assert!(preview.dry_run() && !preview.require_complete());
    assert!(preview.retry_bad() && preview.coverage_all());
    assert!(!complete.dry_run() && complete.require_complete() && complete.retry_bad());
    for policy in [&query, &preview, &complete] {
        assert!(
            !(policy.dry_run() && policy.require_complete()),
            "named modes must not encode dry_run with require_complete"
        );
    }
    assert!(EnsurePolicy::query(true).coverage_all());
    assert_eq!(EnsurePolicy::query(false), EnsurePolicy::complete(false, false));
}

fn workspace_req() -> TargetRequest {
    canonicalize_target_request(
        TargetRequest {
            focus: TargetFocus::Workspace,
            lang: None,
            ignore: Vec::new(),
        },
        None,
    )
}

fn python_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());
    tmp
}

fn seed_population_cache(repo: &std::path::Path) {
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            repo,
            &[],
            &[],
            &[],
        )
    );
    assert!(
        crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
            repo,
            &[],
            &[]
        )
    );
}

#[test]
fn dry_run_preview_does_not_require_complete_membership() {
    let tmp = python_repo();
    let preview = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(preview.deferred || preview.membership_complete);
    assert_eq!(preview.plan.known_execution_union(), Vec::<String>::new());
}

#[test]
fn incomplete_workspace_fails_closed_when_required() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join("test_app.py"),
        "def test_x():\n    assert True\n",
    )
    .unwrap();
    let err = ensure_target_report_query(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::query(false),
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
    .unwrap_err();
    assert!(matches!(err, EnsureError::IncompleteEvidence(_)));
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn query_assembles_a_complete_report_not_yet_in_the_store() {
    let tmp = python_repo();
    let policy = EnsurePolicy::assemble(false);
    let report = assemble_target_report_query(
        tmp.path(),
        &workspace_req(),
        &policy,
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
        .expect("complete cached evidence must assemble without a stored report");
    let super::ensure::Ensured::Report(report) = report;
    assert!(report.stamp.complete);
    assert!(report.rows.is_empty());
}

#[test]
fn query_snapshot_repairs_graph_once() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    super::counters::reset();
    let outcome = super::snapshot::run_snapshot_kernel(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::query(false),
    );
    assert!(
        outcome
            .as_ref()
            .err()
            .is_none_or(|err| matches!(err, EnsureError::IncompleteEvidence(_))),
        "a query either assembles or reports incomplete evidence"
    );
    assert_eq!(super::counters::current().graph, 1);
    let preview = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(!preview.plan.graph_repair);
}

#[test]
fn snapshot_retries_then_returns_concurrent_mutation() {
    let tmp = python_repo();
    let err = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::inject_mismatch_for_test(),
    )
    .unwrap_err();
    assert!(matches!(err, EnsureError::ConcurrentMutation));
    assert_eq!(MAX_SNAPSHOT_ATTEMPTS, 3);
}

#[test]
fn operand_without_evidence_fails_closed() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join("tests_app.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let request = canonicalize_target_request(
        TargetRequest {
            focus: TargetFocus::Operands(vec![OperandExpr {
                raw: "tests_app.py::test_ok".into(),
            }]),
            lang: None,
            ignore: Vec::new(),
        },
        None,
    );
    let err = ensure_target_report_query(
        tmp.path(),
        &request,
        &EnsurePolicy::soft(false, false),
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
    .unwrap_err();
    assert!(matches!(err, EnsureError::IncompleteEvidence(_)));
}

#[test]
fn ensure_target_report_runs_execute_on_miss() {
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::RunTestOnceOutcome;
    use std::sync::atomic::{AtomicBool, Ordering};

    let tmp = python_repo();
    let mut args =
        crate::test_runner::test_mode_fixtures::dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.dry_run = false;
    let ran = AtomicBool::new(false);
    let outcome = ensure_target_report(Some(tmp.path()), &args, true, false, |_a| {
        ran.store(true, Ordering::SeqCst);
        RunTestOnceOutcome::Code(1)
    });
    assert!(ran.load(Ordering::SeqCst));
    match outcome {
        EnsureOutcome::Miss {
            exit_code: 1,
            typed: true,
            engine_aborted: false,
            ..
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_typed_row_fails_closed() {
    let tmp = python_repo();
    let scope = super::scope::ReportScope::from_membership(
        Vec::new(),
        vec!["tests/a.py::test_a".into()],
        true,
    );
    let err = super::rows::rows_from_witnesses(tmp.path(), &scope).unwrap_err();
    assert!(err.contains("tests/a.py::test_a"));
}

#[test]
fn empty_scope_has_no_typed_rows() {
    let tmp = python_repo();
    let scope = super::scope::ReportScope::from_membership(Vec::new(), Vec::new(), true);
    let rows = super::rows::rows_from_witnesses(tmp.path(), &scope).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn workspace_scope_uses_projection_selectors() {
    let tmp = python_repo();
    let preview = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(preview.scope.selectors.is_empty());
    assert_eq!(preview.scope.regions.len(), 1);
}

#[test]
fn successful_report_exits_zero_without_timeouts() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git(&tmp);
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(
        tmp.path().join("tests/test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::soft(false, false),
    )
    .unwrap();
    assert_eq!(report.exit_code, 0);
    assert_eq!(
        super::report::TargetReport::exit_for(Some(super::report::EffectiveStatus::Timeout)),
        1
    );
    assert_eq!(
        super::report::TargetReport::exit_for(Some(super::report::EffectiveStatus::Fail)),
        1
    );
}

#[test]
fn exit_from_rows_prefers_timeout_then_fail() {
    let fail = super::report::SelectorRow {
        language: "python".into(),
        selector: "tests/a.py::test_a".into(),
        raw: "failed".into(),
        effective: super::report::EffectiveStatus::Fail,
        duration_ns: None,
        provenance: "witness".into(),
    };
    let timeout = super::report::SelectorRow {
        language: "python".into(),
        selector: "tests/b.py::test_b".into(),
        raw: "timed_out".into(),
        effective: super::report::EffectiveStatus::Timeout,
        duration_ns: None,
        provenance: "witness".into(),
    };
    assert_eq!(super::report::TargetReport::exit_from_rows(&[]), 0);
    assert_eq!(
        super::report::TargetReport::exit_from_rows(std::slice::from_ref(&fail)),
        1
    );
    assert_eq!(
        super::report::TargetReport::exit_from_rows(&[fail, timeout]),
        1
    );
    assert_eq!(super::report::TargetReport::combine_exit(1, 0), 1);
    assert_eq!(super::report::TargetReport::combine_exit(0, 1), 1);
    assert_eq!(super::report::TargetReport::combine_exit(1, 124), 1);
    assert_eq!(super::report::TargetReport::combine_exit(0, 124), 1);
}

#[test]
fn publish_holds_when_recaptured_rows_match() {
    let tmp = python_repo();
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::soft(false, false),
    )
    .unwrap();
    assert!(!report.evidence.digest.is_empty());
    super::report_store::publish_if_rows_hold(tmp.path(), &workspace_req(), &report).unwrap();
    let recaptured =
        super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap();
    assert_eq!(recaptured.stamp, report.stamp);
    assert_eq!(recaptured.evidence, report.evidence);
    assert!(!report.snapshot.worktree.is_empty());
    assert!(!report.snapshot.gate_policy.is_empty());
    assert_eq!(recaptured.snapshot.worktree, report.snapshot.worktree);
    assert_eq!(recaptured.snapshot.gate_policy, report.snapshot.gate_policy);
    assert!(!report.snapshot.runner.is_empty());
    assert_eq!(recaptured.snapshot.runner, report.snapshot.runner);
    assert_eq!(
        recaptured.snapshot.python_witness,
        report.snapshot.python_witness
    );
    assert_eq!(
        recaptured.snapshot.python_coverage,
        report.snapshot.python_coverage
    );
    assert_eq!(
        recaptured.snapshot.rust_witness,
        report.snapshot.rust_witness
    );
    assert_eq!(
        recaptured.snapshot.rust_coverage,
        report.snapshot.rust_coverage
    );
    assert!(!report.snapshot.resolved.is_empty());
    assert_eq!(recaptured.snapshot.resolved, report.snapshot.resolved);
    assert!(report.snapshot.population.is_some());
    assert_eq!(recaptured.snapshot.population, report.snapshot.population);
    assert!(!report.snapshot.configuration.is_empty());
    assert_eq!(
        recaptured.snapshot.configuration,
        report.snapshot.configuration
    );
    assert!(
        super::report_store::load_report_for_identity(
            tmp.path(),
            &workspace_req(),
            &report.stamp.digest,
            report.stamp.complete,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );
    assert!(
        super::report_store::load_report_for_identity(
            tmp.path(),
            &workspace_req(),
            &report.stamp.digest,
            report.stamp.complete,
            true,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "gate-policy flip must miss"
    );
    assert!(
        super::report_store::load_report_for_identity(
            tmp.path(),
            &workspace_req(),
            "other-digest",
            report.stamp.complete,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none()
    );
    let rust_extra = ["-k".to_string(), "does_not_match".to_string()];
    assert!(
        super::report_store::load_report_for_identity(
            tmp.path(),
            &workspace_req(),
            &report.stamp.digest,
            report.stamp.complete,
            false,
            crate::test_runner::language_keyed::LanguageKeyed {
                python: &[],
                rust: rust_extra.as_slice(),
            },
        )
        .is_none(),
        "runner extra must miss the unfiltered report"
    );
}

#[test]
fn recapture_rejects_worktree_move() {
    let tmp = python_repo();
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::soft(false, false),
    )
    .unwrap();
    super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap();
    fs::write(tmp.path().join("extra.py"), "x = 1\n").unwrap();
    let err =
        super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap_err();
    assert_eq!(err, "concurrent mutation");
}

#[test]
fn recapture_rejects_generation_move() {
    let tmp = python_repo();
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::soft(false, false),
    )
    .unwrap();
    super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap();
    let mut moved = report.clone();
    moved.snapshot.runner = "moved-runner".into();
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.python_witness = Some("moved-python-witness".into());
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.python_coverage = Some("moved-python-coverage".into());
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.rust_coverage = Some("moved-rust-coverage".into());
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.resolved = "moved-resolved".into();
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.population = Some("moved-population".into());
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
    moved = report.clone();
    moved.snapshot.configuration = "moved-configuration".into();
    let err = super::recapture::recapture_report(tmp.path(), &workspace_req(), &moved).unwrap_err();
    assert_eq!(err, "concurrent mutation");
}

#[test]
fn recapture_rejects_population_inventory_move() {
    let tmp = python_repo();
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::soft(false, false),
    )
    .unwrap();
    super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap();
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            tmp.path(),
            &[],
            &["tests/test_moved.py::test_x".into()],
            &[],
        )
    );
    let err =
        super::recapture::recapture_report(tmp.path(), &workspace_req(), &report).unwrap_err();
    assert_eq!(err, "concurrent mutation");
}

#[test]
fn reverse_records_omit_paths_absent_from_index() {
    let tmp = python_repo();
    let records = super::history::reverse_records(tmp.path(), &["pkg/app.py".into()]);
    assert!(records.is_empty());
}

fn sample_row(
    selector: &str,
    effective: super::report::EffectiveStatus,
) -> super::report::SelectorRow {
    super::report::SelectorRow {
        language: "python".into(),
        selector: selector.into(),
        raw: match effective {
            super::report::EffectiveStatus::Pass => "passed",
            super::report::EffectiveStatus::Fail => "failed",
            super::report::EffectiveStatus::Timeout => "timed_out",
        }
        .into(),
        effective,
        duration_ns: None,
        provenance: "witness".into(),
    }
}

#[test]
fn retry_bad_intersects_fail_and_timeout_with_membership() {
    let scope = super::scope::ReportScope::from_membership(
        Vec::new(),
        vec![
            "tests/a.py::test_a".into(),
            "tests/b.py::test_b".into(),
            "tests/c.py::test_c".into(),
        ],
        true,
    );
    let rows = [
        sample_row("tests/a.py::test_a", super::report::EffectiveStatus::Fail),
        sample_row("tests/b.py::test_b", super::report::EffectiveStatus::Pass),
        sample_row(
            "tests/c.py::test_c",
            super::report::EffectiveStatus::Timeout,
        ),
        sample_row(
            "tests/out.py::test_out",
            super::report::EffectiveStatus::Fail,
        ),
    ];
    let plain = super::rows::plan_from_available_rows(&scope, &rows, false, false);
    assert!(plain.retry_bad.is_empty());
    let retry = super::rows::plan_from_available_rows(&scope, &rows, true, false);
    assert_eq!(
        retry.retry_bad,
        vec![
            "tests/a.py::test_a".to_string(),
            "tests/c.py::test_c".to_string()
        ]
    );
    assert!(retry.repair_selectors.is_empty());
}

#[test]
fn retry_bad_defers_when_a_member_row_is_missing() {
    let scope = super::scope::ReportScope::from_membership(
        Vec::new(),
        vec!["tests/a.py::test_a".into(), "tests/b.py::test_b".into()],
        true,
    );
    let rows = [sample_row(
        "tests/a.py::test_a",
        super::report::EffectiveStatus::Fail,
    )];
    let plan = super::rows::plan_from_available_rows(&scope, &rows, true, false);
    assert_eq!(plan.retry_bad, vec!["tests/a.py::test_a".to_string()]);
    assert_eq!(
        plan.repair_selectors,
        vec!["tests/b.py::test_b".to_string()]
    );
}

#[test]
fn duration_incomplete_pass_and_fail_enter_repair() {
    let scope = super::scope::ReportScope::from_membership(
        Vec::new(),
        vec![
            "tests/a.py::test_a".into(),
            "tests/b.py::test_b".into(),
            "tests/c.py::test_c".into(),
        ],
        true,
    );
    let rows = [
        sample_row("tests/a.py::test_a", super::report::EffectiveStatus::Pass),
        sample_row("tests/b.py::test_b", super::report::EffectiveStatus::Fail),
        sample_row(
            "tests/c.py::test_c",
            super::report::EffectiveStatus::Timeout,
        ),
    ];
    let plan = super::rows::plan_from_available_rows_with(&scope, &rows, false, false, false, true);
    assert_eq!(
        plan.repair_selectors,
        vec![
            "tests/a.py::test_a".to_string(),
            "tests/b.py::test_b".to_string()
        ]
    );
    let retry = super::rows::plan_from_available_rows_with(&scope, &rows, true, false, false, true);
    assert_eq!(
        retry.retry_bad,
        vec![
            "tests/b.py::test_b".to_string(),
            "tests/c.py::test_c".to_string()
        ]
    );
    assert!(
        retry
            .repair_selectors
            .contains(&"tests/a.py::test_a".to_string())
    );
}

#[test]
fn force_marks_every_scope_member() {
    let scope = super::scope::ReportScope::from_membership(
        Vec::new(),
        vec!["tests/a.py::test_a".into(), "tests/b.py::test_b".into()],
        true,
    );
    let plan = super::rows::plan_from_available_rows_with(&scope, &[], false, false, true, false);
    assert_eq!(
        plan.forced,
        vec![
            "tests/a.py::test_a".to_string(),
            "tests/b.py::test_b".to_string()
        ]
    );
}

#[test]
fn missing_duration_after_repair_fails_closed() {
    let rows = [sample_row(
        "tests/a.py::test_a",
        super::report::EffectiveStatus::Pass,
    )];
    let err = super::rows::duration_evidence_holds(&rows, true).unwrap_err();
    assert!(err.contains("missing duration"), "{err}");
    assert!(super::rows::duration_evidence_holds(&rows, false).is_ok());
}

#[test]
fn plan_records_graph_repair_flag() {
    let scope = super::scope::ReportScope::from_membership(Vec::new(), Vec::new(), true);
    let on = super::rows::plan_from_available_rows(&scope, &[], false, true);
    let off = super::rows::plan_from_available_rows(&scope, &[], false, false);
    assert!(on.graph_repair);
    assert!(!off.graph_repair);
}

#[test]
fn orphan_config_marks_graph_repair_on_preview() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    let on = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(on.plan.graph_repair);
    let bypass = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, true),
    )
    .unwrap();
    assert!(!bypass.plan.graph_repair);
}

#[test]
fn warm_graph_cache_clears_preview_repair_and_pins_generation() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    let cold = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(cold.plan.graph_repair);
    super::report::repair_graph_evidence(tmp.path(), &cold.scope, false).unwrap();
    let report = super::report::TargetReport::assembled_in(
        tmp.path(),
        &workspace_req(),
        cold.scope.clone(),
        Vec::new(),
        super::slice::TargetSliceStamp {
            digest: "d".into(),
            complete: true,
            index_schema: super::slice::TARGET_SLICE_SCHEMA.into(),
        },
        0,
        false,
    );
    assert!(report.graph_generation.is_some(), "{report:?}");
    let warm = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy::preview(false, false),
    )
    .unwrap();
    assert!(!warm.plan.graph_repair);
}

fn live_policy() -> EnsurePolicy {
    EnsurePolicy::soft(false, false)
}

#[test]
fn snapshot_repairs_graph_before_report_pin() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    super::counters::reset();
    let report = materialize_target_report(tmp.path(), &workspace_req(), &live_policy()).unwrap();
    assert_eq!(super::counters::current().graph, 1);
    assert!(report.graph_generation.is_some(), "{report:?}");
    assert!(!super::report::graph_repair_needed(
        tmp.path(),
        &report.scope,
        false
    ));
    super::counters::reset();
    let again = materialize_target_report(tmp.path(), &workspace_req(), &live_policy()).unwrap();
    assert_eq!(super::counters::current().graph, 0);
    assert_eq!(again.graph_generation, report.graph_generation);
    assert_eq!(report.snapshot.graph_generation, report.graph_generation);
    assert_eq!(report.snapshot.slice, report.stamp);
    assert_eq!(report.snapshot.evidence, report.evidence);
    assert_eq!(again.snapshot.graph_generation, again.graph_generation);
}

#[test]
fn dry_run_preview_does_not_repair_graph() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    super::counters::reset();
    let preview = preview_target_plan_with(tmp.path(), &workspace_req(), &live_policy()).unwrap();
    assert!(preview.plan.graph_repair);
    assert_eq!(super::counters::current().graph, 0);
    assert!(super::report::graph_repair_needed(
        tmp.path(),
        &preview.scope,
        false
    ));
}

#[test]
fn ready_load_misses_when_covered_map_churns_graph_generation() {
    // kt_bug: ready-report identity ignores covered-map ITE key. Watch/oneshot can
    // publish more coverage without changing stamp/request; load_ready must not
    // serve a report whose pinned graph_generation is stale vs current covered.
    let tmp = python_repo();
    fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
    fs::write(tmp.path().join("other.py"), "def unused():\n    return 2\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "sources"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());

    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let mut generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("utils.py".into(), vec![1, 2])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation.clone())
        .unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(built.graph_generation.is_some(), "{built:?}");
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );
    assert!(
        !super::report::graph_repair_needed(tmp.path(), &built.scope, false),
        "published report must leave graph evidence warm under covered map A"
    );

    generation
        .covered_lines
        .insert("other.py".into(), vec![1, 2]);
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();
    assert!(
        super::report::graph_repair_needed(tmp.path(), &built.scope, false),
        "covered-map churn must miss the graph-evidence ITE key"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "ready-load must miss when pinned graph_generation is stale vs current covered map"
    );
}

#[test]
fn ready_load_misses_when_pinned_graph_evidence_blob_is_gone() {
    // kt_bug: pinned_graph_generation_holds also requires the evidence blob to load.
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "orphan-config"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());
    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("app.py".into(), vec![1])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    let key = built
        .graph_generation
        .clone()
        .expect("materialized report must pin a graph generation");
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );

    let dir = tmp
        .path()
        .join("target")
        .join("kiss-plan")
        .join("graph-evidence");
    let full = dir.join(format!("{key}.json"));
    let legacy = dir.join(format!("{}.json", &key[..16.min(key.len())]));
    let _ = fs::remove_file(&full);
    let _ = fs::remove_file(&legacy);
    assert!(
        super::graph_store::load_items(tmp.path(), &key).is_none(),
        "probe: graph evidence blob must be gone for {key}"
    );
    assert!(
        super::report::graph_repair_needed(tmp.path(), &built.scope, false),
        "missing evidence blob must make graph_repair_needed true under the same covered key"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "ready-load must miss when pinned graph_generation key matches but evidence blob is gone"
    );
}


#[test]
fn ready_freshness_after_worktree_skips_evidence_source_redigest() {
    // kt_bug.md: after worktree match, pinned_graph_generation_holds must not
    // re-digest workspace sources; covered/config (+ presence) are the remaining signals.
    let tmp = python_repo();
    fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
    fs::write(tmp.path().join("other.py"), "def unused():\n    return 2\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "sources"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());

    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let mut generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("utils.py".into(), vec![1, 2])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation.clone())
        .unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(built.graph_generation.is_some(), "{built:?}");
    assert!(
        built.snapshot.graph_mutable.is_some(),
        "assembled reports must pin graph_mutable for cheap ready freshness: {built:?}"
    );
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();

    super::graph_store::reset_evidence_source_reads();
    assert!(
        super::report::pinned_graph_generation_holds(tmp.path(), false, &built),
        "warm pinned generation must hold under stable covered/config"
    );
    assert_eq!(
        super::graph_store::evidence_source_reads(),
        0,
        "after worktree-validated sources, ready freshness must not re-digest evidence sources"
    );
    super::graph_store::reset_evidence_source_reads();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );
    assert_eq!(
        super::graph_store::evidence_source_reads(),
        0,
        "full load_ready_for_request warm path must not re-digest evidence sources"
    );

    generation
        .covered_lines
        .insert("other.py".into(), vec![1, 2]);
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();
    super::graph_store::reset_evidence_source_reads();
    assert!(
        !super::report::pinned_graph_generation_holds(tmp.path(), false, &built),
        "covered-map churn must miss without relying on source re-digest"
    );
    assert_eq!(
        super::graph_store::evidence_source_reads(),
        0,
        "covered-map miss path must also skip evidence source re-digest"
    );
}


#[test]
fn ready_misses_when_gitignored_rust_include_target_changes() {
    // kt_bug.md: evidence_key expands include! targets (even gitignored); worktree must
    // not warm-hold after those bytes change.
    let tmp = python_repo();
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\ngen.rs\n").unwrap();
    fs::write(
        tmp.path().join("lib.rs"),
        "include!(\"gen.rs\");\npub fn prod() -> i32 { included() }\n",
    )
    .unwrap();
    fs::write(tmp.path().join("gen.rs"), "fn included() -> i32 { 1 }\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "rs-include"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());

    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for include lock: {built:?}"
    );
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "warm ready must hit before include-target edit"
    );

    let before_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    fs::write(tmp.path().join("gen.rs"), "fn included() -> i32 { 2 }\n").unwrap();
    let after_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    assert_ne!(
        before_wt, after_wt,
        "worktree token must move when a gitignored include! target changes"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "gitignored include! target edit must miss ready"
    );
}

#[test]
fn ready_misses_when_gitignored_rust_path_attr_target_changes() {
    // kt_bug.md class #8: evidence_key / worktree must follow #[path] targets
    // (including gitignored), same premise as the include! lock above.
    let tmp = python_repo();
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\nalt.rs\n").unwrap();
    fs::write(
        tmp.path().join("lib.rs"),
        "#[path = \"alt.rs\"]\nmod hidden;\npub fn prod() -> i32 { hidden::via_path() }\n",
    )
    .unwrap();
    fs::write(tmp.path().join("alt.rs"), "pub fn via_path() -> i32 { 1 }\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "rs-path-attr"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());

    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2, 3])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for path-attr lock: {built:?}"
    );
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "warm ready must hit before path-attr target edit"
    );

    let before_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    fs::write(tmp.path().join("alt.rs"), "pub fn via_path() -> i32 { 2 }\n").unwrap();
    let after_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    assert_ne!(
        before_wt, after_wt,
        "worktree token must move when a gitignored #[path] target changes"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "gitignored #[path] target edit must miss ready"
    );
}

#[test]
fn ready_misses_when_gitignored_rust_conventional_mod_target_changes() {
    // kt_bug.md class #9: evidence_key / worktree must follow conventional
    // `mod name;` targets (name.rs / name/mod.rs), including gitignored.
    let tmp = python_repo();
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\nhidden.rs\n").unwrap();
    fs::write(
        tmp.path().join("lib.rs"),
        "mod hidden;\npub fn prod() -> i32 { hidden::via_conv() }\n",
    )
    .unwrap();
    fs::write(tmp.path().join("hidden.rs"), "pub fn via_conv() -> i32 { 1 }\n").unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "rs-conventional-mod"])
            .status()
            .unwrap()
            .success()
    );
    seed_population_cache(tmp.path());

    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(tmp.path());
    let generation = crate::test_runner::execution_generation::FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: vec!["a".into()],
        selector_evidence: vec![crate::test_runner::execution_generation::SelectorEvidenceRecord {
            selector: "a".into(),
            raw_status: "passed".into(),
            duration_ns: Some(1),
            entry_content_digest: "blob-a".into(),
            evidence_state: "valid".into(),
            ..Default::default()
        }],
        functional_summary_all_pass: true,
        covered_lines: std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2])]),
        ..Default::default()
    };
    crate::test_runner::execution_generation::publish_full_generation(&cache, generation).unwrap();

    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for conventional-mod lock: {built:?}"
    );
    crate::test_runner::target_request::publish_report(tmp.path(), &req, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "warm ready must hit before conventional-mod target edit"
    );

    let before_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    fs::write(tmp.path().join("hidden.rs"), "pub fn via_conv() -> i32 { 2 }\n").unwrap();
    let after_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    assert_ne!(
        before_wt, after_wt,
        "worktree token must move when a gitignored conventional mod target changes"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "gitignored conventional mod target edit must miss ready"
    );
}

#[test]
fn lang_ready_load_does_not_slice_parent_workspace_report() {
    let tmp = python_repo();
    let parent = workspace_req();
    let built = materialize_target_report(tmp.path(), &parent, &live_policy()).unwrap();
    crate::test_runner::target_request::publish_if_rows_hold(tmp.path(), &parent, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &parent,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );
    let mut rust = parent.clone();
    rust.lang = Some(super::types::LangFilter::Rust);
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &rust,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "language-filtered ready-load must not slice a parent workspace report"
    );
}

#[test]
fn ready_load_distinguishes_python_extras() {
    let tmp = python_repo();
    let parent = workspace_req();
    let mut built = materialize_target_report(tmp.path(), &parent, &live_policy()).unwrap();
    built.snapshot.extras = crate::test_runner::language_keyed::LanguageKeyed {
        python: vec!["-k".into(), "foo".into()],
        rust: Vec::new(),
    };
    crate::test_runner::target_request::publish_report(tmp.path(), &parent, &built).unwrap();

    let py_foo = vec!["-k".to_string(), "foo".to_string()];
    let py_bar = vec!["-k".to_string(), "bar".to_string()];
    let match_extras = crate::test_runner::language_keyed::LanguageKeyed {
        python: py_foo.as_slice(),
        rust: &[][..],
    };
    let other_extras = crate::test_runner::language_keyed::LanguageKeyed {
        python: py_bar.as_slice(),
        rust: &[][..],
    };
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &parent,
            false,
            match_extras,
        )
        .is_some(),
        "same language-keyed extras must reuse the ready report"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &parent,
            false,
            other_extras,
        )
        .is_none(),
        "different python extras must not reuse a ready report"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &parent,
            false,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "empty extras must not reuse a report keyed with python extras"
    );
}
