use super::publish_merge::{
    covered_sets_for_publish, merge_statuses, publication_universe, publish_complete,
    statuses_from_summary,
};
use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, PublishBatch, WitnessScope, WitnessStatus,
};
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};
use kiss::rpytest_runner::TestStatus;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

fn witness(complete: bool) -> ExecutionWitness {
    ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: "id".into(),
        selectors: vec!["a".into(), "b".into()],
        statuses: vec![WitnessStatus::Passed, WitnessStatus::Failed],
        durations_ns: vec![Some(1), Some(2)],
        covered_lines: BTreeMap::from([("f.rs".into(), vec![1])]),
        complete,
        generation_id: "g".into(),
        raw_statuses: Vec::new(),
    }
}

#[test]
fn merge_updates_only_ran_selectors_and_preserves_siblings() {
    let prior = witness(false);
    let batch = PublishBatch {
        selectors: vec!["b".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(9)],
        covered_lines: BTreeMap::new(),
        publication_universe: Some(vec!["a".into(), "b".into()]),
        summary: SelectorExecutionSummary::default(),
    };
    let universe = publication_universe(&batch, Some(&prior));
    let (statuses, durations) = merge_statuses(&universe, Some(&prior), &batch);
    assert_eq!(statuses, vec![WitnessStatus::Passed, WitnessStatus::Passed]);
    assert_eq!(durations, vec![Some(1), Some(9)]);
    let covered = covered_sets_for_publish(&batch, Some(&prior));
    assert!(covered.contains_key("f.rs"));
    let req = EnsureRequest {
        repo_root: PathBuf::from("/tmp"),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: universe.clone(),
        },
    };
    assert!(publish_complete(&req, &universe, &statuses, Some(&prior)));
}

#[test]
fn subset_publication_cannot_drop_full_siblings() {
    let prior = witness(true);
    let batch = PublishBatch {
        selectors: vec!["b".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(9)],
        covered_lines: BTreeMap::new(),
        publication_universe: Some(vec!["b".into()]),
        summary: SelectorExecutionSummary::default(),
    };
    let universe = publication_universe(&batch, Some(&prior));
    assert_eq!(universe, vec!["a".to_string(), "b".to_string()]);
    let (statuses, _) = merge_statuses(&universe, Some(&prior), &batch);
    assert_eq!(statuses, vec![WitnessStatus::Passed, WitnessStatus::Passed]);
}

#[test]
fn empty_prior_baseline_is_unresolved() {
    let batch = PublishBatch {
        selectors: vec!["a".into()],
        statuses: vec![WitnessStatus::Passed],
        durations_ns: vec![Some(1)],
        covered_lines: BTreeMap::from([("x.rs".into(), vec![2])]),
        publication_universe: None,
        summary: SelectorExecutionSummary::default(),
    };
    let universe = publication_universe(&batch, None);
    assert_eq!(universe, vec!["a".to_string()]);
    let (statuses, _) = merge_statuses(&universe, None, &batch);
    assert_eq!(statuses, vec![WitnessStatus::Passed]);
    let covered = covered_sets_for_publish(&batch, None);
    assert_eq!(covered["x.rs"].iter().copied().collect::<Vec<_>>(), vec![2]);
}

#[test]
fn statuses_from_summary_classifies_failed_and_timeout() {
    let mut summary = SelectorExecutionSummary {
        failed_selectors: vec!["f".into()],
        timed_out_selectors: vec!["t".into()],
        ..Default::default()
    };
    summary.selector_durations_ns.insert("p".into(), 3);
    let (st, dur) = statuses_from_summary(&summary, &["p".into(), "f".into(), "t".into()]);
    assert_eq!(
        st,
        vec![
            WitnessStatus::Passed,
            WitnessStatus::Failed,
            WitnessStatus::TimedOut
        ]
    );
    assert_eq!(dur, vec![Some(3), None, None]);
}

#[test]
fn statuses_from_summary_prefers_raw_over_effective_sla() {
    let mut summary = SelectorExecutionSummary::default();
    summary.record(SelectorExecutionRecord {
        selector: "slow_but_passed".into(),
        status: TestStatus::TimedOut,
        raw_status: Some(TestStatus::Passed),
        cache_record: SelectorCacheRecord::MissStored,
        exit_code: Some(124),
        duration: Duration::from_secs(2),
    });
    assert_eq!(
        summary.timed_out_selectors,
        vec!["slow_but_passed".to_string()]
    );
    let (st, _) = statuses_from_summary(&summary, &["slow_but_passed".into()]);
    assert_eq!(st, vec![WitnessStatus::Passed]);
}
