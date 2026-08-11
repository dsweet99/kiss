//! Merge helpers for Rust ensure-publish.

use std::collections::{BTreeMap, BTreeSet};

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, PublishBatch, WitnessStatus,
};

pub(super) fn publication_universe(
    batch: &PublishBatch,
    prior: Option<&ExecutionWitness>,
) -> Vec<String> {
    batch
        .publication_universe
        .clone()
        .or_else(|| prior.map(|w| w.selectors.clone()))
        .unwrap_or_else(|| batch.selectors.clone())
}

pub(super) fn merge_statuses(
    universe: &[String],
    prior: Option<&ExecutionWitness>,
    batch: &PublishBatch,
) -> (Vec<WitnessStatus>, Vec<u64>) {
    let (mut statuses, mut durations) = baseline_from_prior(universe, prior);
    let ran_index: BTreeMap<&str, usize> = batch
        .selectors
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let uni_index: BTreeMap<&str, usize> = universe
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    for (sel, &ri) in &ran_index {
        if let Some(&ui) = uni_index.get(sel) {
            statuses[ui] = batch.statuses[ri];
            durations[ui] = batch.durations_ns[ri];
        }
    }
    (statuses, durations)
}

fn baseline_from_prior(
    universe: &[String],
    prior: Option<&ExecutionWitness>,
) -> (Vec<WitnessStatus>, Vec<u64>) {
    let Some(w) = prior else {
        return (
            vec![WitnessStatus::Unresolved; universe.len()],
            vec![0u64; universe.len()],
        );
    };
    let index: BTreeMap<&str, usize> = w
        .selectors
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let mut st = Vec::with_capacity(universe.len());
    let mut dur = Vec::with_capacity(universe.len());
    for sel in universe {
        if let Some(&i) = index.get(sel.as_str()) {
            st.push(w.statuses[i]);
            dur.push(w.durations_ns[i]);
        } else {
            st.push(WitnessStatus::Unresolved);
            dur.push(0);
        }
    }
    (st, dur)
}

pub(super) fn covered_sets_for_publish(
    batch: &PublishBatch,
    prior: Option<&ExecutionWitness>,
) -> BTreeMap<String, BTreeSet<u32>> {
    if batch.covered_lines.is_empty() {
        return prior
            .map(|w| {
                w.covered_lines
                    .iter()
                    .map(|(k, v)| (k.clone(), v.iter().copied().collect()))
                    .collect()
            })
            .unwrap_or_default();
    }
    batch
        .covered_lines
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().copied().collect()))
        .collect()
}

pub(super) fn publish_complete(
    request: &EnsureRequest,
    universe: &[String],
    statuses: &[WitnessStatus],
    prior: Option<&ExecutionWitness>,
) -> bool {
    statuses.iter().all(|s| *s == WitnessStatus::Passed)
        && match request.mode {
            AcceptMode::All => universe.len() == request.planned_rust.len(),
            AcceptMode::Subset => prior.is_some_and(|w| w.complete),
        }
}

pub(super) fn statuses_from_summary(
    summary: &crate::test_runner::runners::SelectorExecutionSummary,
    selectors: &[String],
) -> (Vec<WitnessStatus>, Vec<u64>) {
    let failed: std::collections::BTreeSet<_> =
        summary.failed_selectors.iter().cloned().collect();
    let timed: std::collections::BTreeSet<_> =
        summary.timed_out_selectors.iter().cloned().collect();
    let mut statuses = Vec::with_capacity(selectors.len());
    let mut durations = Vec::with_capacity(selectors.len());
    for sel in selectors {
        let status = if timed.contains(sel) {
            WitnessStatus::TimedOut
        } else if failed.contains(sel) {
            WitnessStatus::Failed
        } else {
            WitnessStatus::Passed
        };
        statuses.push(status);
        durations.push(
            summary
                .selector_durations_ns
                .get(sel)
                .copied()
                .unwrap_or(0),
        );
    }
    (statuses, durations)
}
