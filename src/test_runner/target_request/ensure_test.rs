use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use std::fs;

use super::canon::canonicalize_target_request;
use super::ensure::{
    EnsureOutcome, assemble_target_report_query, ensure_target_report, ensure_target_report_query,
    materialize_target_report, preview_target_plan_with,
};
use super::snapshot::{EnsureError, EnsurePolicy, MAX_SNAPSHOT_ATTEMPTS};
use super::types::{OperandExpr, TargetFocus, TargetRequest};

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
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: true,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
        &[],
    )
    .unwrap_err();
    assert!(matches!(err, EnsureError::IncompleteEvidence(_)));
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn query_assembles_a_complete_report_not_yet_in_the_store() {
    let tmp = python_repo();
    let policy = EnsurePolicy {
        dry_run: false,
        require_complete: false,
        inject_mismatch: false,
        retry_bad: false,
        coverage_all: false,
        assemble_only: true,
    };
    let report = assemble_target_report_query(tmp.path(), &workspace_req(), &policy, &[])
        .expect("complete cached evidence must assemble without a stored report");
    let super::ensure::Ensured::Report(report) = report;
    assert!(report.stamp.complete);
    assert!(report.rows.is_empty());
}

#[test]
fn incomplete_membership_still_repairs_graph() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    super::counters::reset();
    let err = match super::snapshot::run_snapshot_kernel(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy {
            dry_run: false,
            require_complete: true,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    ) {
        Err(err) => err,
        Ok(_) => panic!("expected incomplete membership"),
    };
    assert!(matches!(err, EnsureError::IncompleteEvidence(_)));
    assert_eq!(super::counters::current().graph, 1);
    let preview = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: true,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
        &[],
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
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    assert_eq!(report.exit_code, 0);
    assert_eq!(
        super::report::TargetReport::exit_for(Some(super::report::EffectiveStatus::Timeout)),
        124
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
        124
    );
    assert_eq!(super::report::TargetReport::combine_exit(1, 0), 1);
    assert_eq!(super::report::TargetReport::combine_exit(0, 1), 1);
    assert_eq!(super::report::TargetReport::combine_exit(1, 124), 124);
}

#[test]
fn publish_holds_when_recaptured_rows_match() {
    let tmp = python_repo();
    let report = materialize_target_report(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
            &[],
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
            &[],
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
            &[],
        )
        .is_none()
    );
    assert!(
        super::report_store::load_report_for_identity(
            tmp.path(),
            &workspace_req(),
            &report.stamp.digest,
            report.stamp.complete,
            false,
            &["-k".into(), "does_not_match".into()],
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    assert!(on.plan.graph_repair);
    let bypass = preview_target_plan_with(
        tmp.path(),
        &workspace_req(),
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: true,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
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
        &EnsurePolicy {
            dry_run: true,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    assert!(!warm.plan.graph_repair);
}

fn live_policy() -> EnsurePolicy {
    EnsurePolicy {
        dry_run: false,
        require_complete: false,
        inject_mismatch: false,
        retry_bad: false,
        coverage_all: false,
        assemble_only: false,
    }
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
fn lang_ready_load_does_not_slice_parent_workspace_report() {
    let tmp = python_repo();
    let parent = workspace_req();
    let built = materialize_target_report(tmp.path(), &parent, &live_policy()).unwrap();
    crate::test_runner::target_request::publish_if_rows_hold(tmp.path(), &parent, &built).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(tmp.path(), &parent, false, &[])
            .is_some()
    );
    let mut rust = parent.clone();
    rust.lang = Some(super::types::LangFilter::Rust);
    assert!(
        crate::test_runner::target_request::load_ready_for_request(tmp.path(), &rust, false, &[])
            .is_none(),
        "language-filtered ready-load must not slice a parent workspace report"
    );
}
