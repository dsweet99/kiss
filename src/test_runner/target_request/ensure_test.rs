use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use std::fs;

use super::canon::canonicalize_target_request;
use super::ensure::{
    EnsureOutcome, ensure_target_report, ensure_target_report_query, materialize_target_report,
    preview_target_plan_with,
};
use super::snapshot::{EnsureError, EnsurePolicy, MAX_SNAPSHOT_ATTEMPTS};
use super::types::{OperandExpr, TargetFocus, TargetRequest};

#[test]
fn ensure_policy_named_modes_exclude_invalid_dry_run_require_complete() {
    // PWS: free bool bags allowed dry_run∧require_complete; named modes must not.
    let query = EnsurePolicy::complete(false);
    let preview = EnsurePolicy::preview(true);
    let complete = EnsurePolicy::complete(true);
    assert!(!query.dry_run() && query.require_complete());
    assert!(!query.retry_bad() && !query.inject_mismatch() && !query.assemble_only());
    assert!(preview.dry_run() && !preview.require_complete());
    assert!(preview.retry_bad());
    assert!(!complete.dry_run() && complete.require_complete() && complete.retry_bad());
    for policy in [&query, &preview, &complete] {
        assert!(
            !(policy.dry_run() && policy.require_complete()),
            "named modes must not encode dry_run with require_complete"
        );
    }
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

/// Makes the Rust test `a` a member, so a ready report has a row to derive.
fn seed_rust_member(repo: &std::path::Path) {
    assert!(
        crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
            repo,
            &[],
            &["a".into()]
        )
    );
    crate::test_runner::lang_rust::test_records::store(
        repo,
        &[(
            "a".into(),
            crate::test_runner::lang_iface::WitnessStatus::Passed,
        )],
    );
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
    let preview =
        preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
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
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
    .unwrap_err();
    assert!(matches!(err, EnsureError::IncompleteEvidence(_)));
    assert_eq!(err.exit_code(), 1);
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
        &EnsurePolicy::complete(false),
    );
    assert!(
        outcome
            .as_ref()
            .err()
            .is_none_or(|err| matches!(err, EnsureError::IncompleteEvidence(_))),
        "a query either assembles or reports incomplete evidence"
    );
    assert_eq!(super::counters::current().graph, 1);
    let preview =
        preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
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
    assert!(
        crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
            tmp.path(),
            &[],
            &["a".into()]
        ),
        "a member without a record must run"
    );
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
    let err = super::rows::rows_from_witnesses(
        tmp.path(),
        &scope,
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
    .unwrap_err();
    assert!(err.contains("tests/a.py::test_a"));
}

#[test]
fn empty_scope_has_no_typed_rows() {
    let tmp = python_repo();
    let scope = super::scope::ReportScope::from_membership(Vec::new(), Vec::new(), true);
    let rows = super::rows::rows_from_witnesses(
        tmp.path(),
        &scope,
        crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
    )
    .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn workspace_scope_uses_projection_selectors() {
    let tmp = python_repo();
    let preview =
        preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
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
    let report =
        materialize_target_report(tmp.path(), &workspace_req(), &EnsurePolicy::soft(false))
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
fn assembled_exit_keeps_runner_failure_when_rows_pass() {
    use super::report::EffectiveStatus;
    use super::snapshot::assembled_exit;
    let pass = [sample_row("tests/a.py::test_a", EffectiveStatus::Pass)];
    let fail = [sample_row("tests/a.py::test_a", EffectiveStatus::Fail)];
    assert_eq!(assembled_exit(&pass, 0), 0);
    assert_eq!(assembled_exit(&pass, 1), 1);
    assert_eq!(assembled_exit(&pass, 124), 1);
    assert_eq!(assembled_exit(&pass, 130), 130);
    assert_eq!(assembled_exit(&fail, 0), 1);
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
    let on = preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
        .unwrap();
    assert!(on.plan.graph_repair);
}

#[test]
fn warm_graph_cache_clears_preview_repair_and_pins_generation() {
    let tmp = python_repo();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\norphan_detection = true\n",
    )
    .unwrap();
    let cold =
        preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
            .unwrap();
    assert!(cold.plan.graph_repair);
    super::report::repair_graph_evidence(tmp.path(), &cold.scope).unwrap();
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
    );
    assert!(report.graph_generation.is_some(), "{report:?}");
    let warm =
        preview_target_plan_with(tmp.path(), &workspace_req(), &EnsurePolicy::preview(false))
            .unwrap();
    assert!(!warm.plan.graph_repair);
}

fn live_policy() -> EnsurePolicy {
    EnsurePolicy::soft(false)
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
        &report.scope
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
        &preview.scope
    ));
}

#[test]
fn ready_load_misses_when_covered_map_churns_graph_generation() {
    // kt_bug: ready-report identity ignores covered-map ITE key. A run can
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

    let mut covered: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::from([("utils.py".into(), vec![1, 2])]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);

    seed_rust_member(tmp.path());
    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(built.graph_generation.is_some(), "{built:?}");
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );
    assert!(
        !super::report::graph_repair_needed(tmp.path(), &built.scope),
        "published report must leave graph evidence warm under covered map A"
    );

    covered.insert("other.py".into(), vec![1, 2]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);
    assert!(
        super::report::graph_repair_needed(tmp.path(), &built.scope),
        "covered-map churn must miss the graph-evidence ITE key"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "ready-load must miss when pinned graph_generation is stale vs current covered map"
    );
}

#[test]
fn ready_load_misses_when_pinned_graph_evidence_blob_is_gone() {
    // kt_bug: a ready report also requires the graph evidence blob to load.
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
    let covered: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::from([("app.py".into(), vec![1])]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);

    seed_rust_member(tmp.path());
    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    let key = built
        .graph_generation
        .clone()
        .expect("materialized report must pin a graph generation");
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some()
    );

    let dir = tmp.path().join(".kiss").join("test").join("graph-evidence");
    let _ = fs::remove_file(dir.join(format!("{key}.json")));
    assert!(
        super::graph_store::load_items(tmp.path(), &key).is_none(),
        "probe: graph evidence blob must be gone for {key}"
    );
    assert!(
        super::report::graph_repair_needed(tmp.path(), &built.scope),
        "missing evidence blob must make graph_repair_needed true under the same covered key"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "ready-load must miss when pinned graph_generation key matches but evidence blob is gone"
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

    let covered: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2])]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);

    seed_rust_member(tmp.path());
    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for include lock: {built:?}"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
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
    fs::write(
        tmp.path().join("alt.rs"),
        "pub fn via_path() -> i32 { 1 }\n",
    )
    .unwrap();
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

    let covered: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2, 3])]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);

    seed_rust_member(tmp.path());
    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for path-attr lock: {built:?}"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "warm ready must hit before path-attr target edit"
    );

    let before_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    fs::write(
        tmp.path().join("alt.rs"),
        "pub fn via_path() -> i32 { 2 }\n",
    )
    .unwrap();
    let after_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    assert_ne!(
        before_wt, after_wt,
        "worktree token must move when a gitignored #[path] target changes"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
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
    fs::write(
        tmp.path().join(".gitignore"),
        "/target\n/.kiss\nhidden.rs\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("lib.rs"),
        "mod hidden;\npub fn prod() -> i32 { hidden::via_conv() }\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("hidden.rs"),
        "pub fn via_conv() -> i32 { 1 }\n",
    )
    .unwrap();
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

    let covered: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::from([("lib.rs".into(), vec![1, 2])]);
    crate::test_runner::lang_python::store_test_record_covering(tmp.path(), "a", &covered);

    seed_rust_member(tmp.path());
    let req = workspace_req();
    let built = materialize_target_report(tmp.path(), &req, &live_policy()).unwrap();
    assert!(
        built.graph_generation.is_some(),
        "orphan graph must be active for conventional-mod lock: {built:?}"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "warm ready must hit before conventional-mod target edit"
    );

    let before_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    fs::write(
        tmp.path().join("hidden.rs"),
        "pub fn via_conv() -> i32 { 2 }\n",
    )
    .unwrap();
    let after_wt = super::stamp::capture_worktree_token(tmp.path(), None);
    assert_ne!(
        before_wt, after_wt,
        "worktree token must move when a gitignored conventional mod target changes"
    );
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            tmp.path(),
            &req,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_none(),
        "gitignored conventional mod target edit must miss ready"
    );
}
