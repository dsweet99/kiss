//! Unit tests for shared `accept_witness`.

use kiss::GateConfig;

use super::witness::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
    miss_selectors_for_repair, reclassify_statuses_with_gate,
};

fn witness(
    scope: WitnessScope,
    identity: &str,
    selectors: &[&str],
    statuses: &[WitnessStatus],
    complete: bool,
) -> ExecutionWitness {
    ExecutionWitness {
        language: "rust".into(),
        scope,
        identity_digest: identity.into(),
        selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        statuses: statuses.to_vec(),
        durations_ns: vec![Some(1_000_000); selectors.len()],
        covered_lines: Default::default(),
        complete,
        generation_id: "gen-1".into(),
    }
}

#[test]
fn all_mode_accepts_full_complete_equality() {
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["b".into(), "a".into()], "id", &w),
        AcceptDecision::Accept
    );
}

#[test]
fn all_mode_rejects_subset_scope() {
    let w = witness(
        WitnessScope::Subset,
        "id",
        &["a"],
        &[WitnessStatus::Passed],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into()], "id", &w),
        AcceptDecision::Miss("scope_subset")
    );
}

#[test]
fn all_mode_rejects_identity_mismatch() {
    let w = witness(
        WitnessScope::Full,
        "old",
        &["a"],
        &[WitnessStatus::Passed],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into()], "new", &w),
        AcceptDecision::Miss("identity")
    );
}

#[test]
fn all_mode_rejects_selector_lag() {
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        true,
    );
    assert_eq!(
        accept_witness(
            AcceptMode::All,
            &["a".into(), "b".into(), "c".into()],
            "id",
            &w
        ),
        AcceptDecision::Miss("selector_universe")
    );
}

#[test]
fn all_mode_rejects_incomplete_and_failed() {
    let incomplete = witness(
        WitnessScope::Full,
        "id",
        &["a"],
        &[WitnessStatus::Passed],
        false,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into()], "id", &incomplete),
        AcceptDecision::Miss("incomplete")
    );
    let failed = witness(
        WitnessScope::Full,
        "id",
        &["a"],
        &[WitnessStatus::Failed],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into()], "id", &failed),
        AcceptDecision::Miss("non_passed")
    );
}

#[test]
fn subset_mode_accepts_membership_under_full_or_subset() {
    let full = witness(
        WitnessScope::Full,
        "id",
        &["a", "b", "c"],
        &[
            WitnessStatus::Passed,
            WitnessStatus::Passed,
            WitnessStatus::Passed,
        ],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::Subset, &["b".into()], "id", &full),
        AcceptDecision::Accept
    );
    let subset = witness(
        WitnessScope::Subset,
        "id",
        &["b", "c"],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        false,
    );
    assert_eq!(
        accept_witness(AcceptMode::Subset, &["c".into()], "id", &subset),
        AcceptDecision::Accept
    );
}

#[test]
fn subset_mode_rejects_missing_or_failed() {
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Failed],
        true,
    );
    assert_eq!(
        accept_witness(AcceptMode::Subset, &["z".into()], "id", &w),
        AcceptDecision::Miss("missing_selector")
    );
    assert_eq!(
        accept_witness(AcceptMode::Subset, &["b".into()], "id", &w),
        AcceptDecision::Miss("non_passed")
    );
}

#[test]
fn time_limit_reclassify_forces_miss_on_affected_selector() {
    let gate = GateConfig {
        max_unit_test_seconds: vec![("*".into(), 0.0)],
        ..GateConfig::default()
    };
    let effective = reclassify_statuses_with_gate(
        &["a".into(), "b".into()],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        &[Some(1_000_000_000), Some(0)],
        &gate,
    );
    assert_eq!(
        effective,
        vec![WitnessStatus::TimedOut, WitnessStatus::TimedOut]
    );
    let mut w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &effective,
        true,
    );
    w.statuses = effective;
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into(), "b".into()], "id", &w),
        AcceptDecision::Miss("non_passed")
    );
}

#[test]
fn warm_accept_reclassify_applies_tighter_session_gate() {
    // Stored Passed + long duration must become TimedOut when the current gate
    // tightens — ownership of time-limit policy on the accept/warm side.
    let loose = GateConfig {
        max_unit_test_seconds: vec![("*".into(), 3600.0)],
        ..GateConfig::default()
    };
    let tight = GateConfig {
        max_unit_test_seconds: vec![("*".into(), 0.5)],
        ..GateConfig::default()
    };
    let under_loose = reclassify_statuses_with_gate(
        &["tests/a.py::t".into()],
        &[WitnessStatus::Passed],
        &[Some(2_000_000_000)], // 2s
        &loose,
    );
    assert_eq!(under_loose, vec![WitnessStatus::Passed]);
    let under_tight = reclassify_statuses_with_gate(
        &["tests/a.py::t".into()],
        &[WitnessStatus::Passed],
        &[Some(2_000_000_000)],
        &tight,
    );
    assert_eq!(under_tight, vec![WitnessStatus::TimedOut]);
}

#[test]
fn missing_duration_fails_closed_under_active_time_gate() {
    let gate = GateConfig {
        max_unit_test_seconds: vec![("*".into(), 60.0)],
        ..GateConfig::default()
    };
    let effective = reclassify_statuses_with_gate(
        &["a".into()],
        &[WitnessStatus::Passed],
        &[None],
        &gate,
    );
    assert_eq!(effective, vec![WitnessStatus::Failed]);
    let disabled = GateConfig {
        max_unit_test_seconds: vec![],
        ..GateConfig::default()
    };
    let kept = reclassify_statuses_with_gate(
        &["a".into()],
        &[WitnessStatus::Passed],
        &[None],
        &disabled,
    );
    assert_eq!(kept, vec![WitnessStatus::Passed]);
}

#[test]
fn shape_mismatch_rejects() {
    let mut w = witness(
        WitnessScope::Full,
        "id",
        &["a"],
        &[WitnessStatus::Passed],
        true,
    );
    w.statuses.push(WitnessStatus::Passed);
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into()], "id", &w),
        AcceptDecision::Miss("witness_shape")
    );
}

#[test]
fn incomplete_full_miss_repairs_non_passed_not_scope() {
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Failed],
        false,
    );
    assert_eq!(
        accept_witness(AcceptMode::All, &["a".into(), "b".into()], "id", &w),
        AcceptDecision::Miss("incomplete")
    );
    assert_eq!(
        miss_selectors_for_repair(
            AcceptMode::All,
            &["a".into(), "b".into()],
            "id",
            Some(&w),
            false,
        ),
        vec!["b".to_string()]
    );
}

#[test]
fn force_and_missing_witness_run_all_planned() {
    let planned = vec!["a".into(), "b".into()];
    assert_eq!(
        miss_selectors_for_repair(AcceptMode::All, &planned, "id", None, false),
        planned
    );
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        true,
    );
    assert_eq!(
        miss_selectors_for_repair(AcceptMode::All, &planned, "id", Some(&w), true),
        planned
    );
    assert!(
        miss_selectors_for_repair(AcceptMode::All, &planned, "id", Some(&w), false).is_empty()
    );
}

#[test]
fn force_selectors_invalidate_stale_passed_without_batch_force() {
    let planned = vec!["a".into(), "b".into()];
    let w = witness(
        WitnessScope::Full,
        "id",
        &["a", "b"],
        &[WitnessStatus::Passed, WitnessStatus::Passed],
        true,
    );
    let mut misses =
        miss_selectors_for_repair(AcceptMode::Subset, &planned, "id", Some(&w), false);
    assert!(misses.is_empty(), "stale Passed witness would warm-accept");
    crate::test_runner::lang_iface::union_force_selectors_into_misses(
        &planned,
        &mut misses,
        &["a".into()],
    );
    assert_eq!(misses, vec!["a".to_string()]);
    assert!(
        miss_selectors_for_repair(AcceptMode::Subset, &planned, "id", Some(&w), false).is_empty(),
        "batch force remains false: other selectors stay warm-eligible"
    );
}
