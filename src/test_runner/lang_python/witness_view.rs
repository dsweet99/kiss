use std::path::Path;

use kiss::GateConfig;

use crate::test_runner::lang_iface::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
    reclassify_statuses_with_gate, summary_from_accepted_witness,
};
use crate::test_runner::python_coverage_index::generation::{
    PinnedPythonGeneration, identity_matches_current, try_load_pinned_python_generation_warm,
};
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) fn python_identity_digest(pinned: &PinnedPythonGeneration) -> String {
    format!(
        "py:{}:{}",
        pinned.plan.base_identity.input_fingerprint, pinned.generation_id
    )
}

pub(crate) fn python_witness_from_pinned(pinned: &PinnedPythonGeneration) -> ExecutionWitness {
    let mut by_sel: std::collections::BTreeMap<String, (WitnessStatus, Option<u64>)> = pinned
        .timings
        .iter()
        .map(|t| {
            (
                t.selector.clone(),
                (WitnessStatus::parse(&t.raw_status), t.duration_ns),
            )
        })
        .collect();
    let mut selectors = pinned.plan.selectors.clone();
    selectors.sort();
    selectors.dedup();
    let mut statuses = Vec::with_capacity(selectors.len());
    let mut durations_ns = Vec::with_capacity(selectors.len());
    for sel in &selectors {
        match by_sel.remove(sel) {
            Some((st, d)) => {
                statuses.push(st);
                durations_ns.push(d);
            }
            None => {
                statuses.push(WitnessStatus::Unresolved);
                durations_ns.push(None);
            }
        }
    }

    ExecutionWitness {
        language: "python".into(),
        scope: WitnessScope::Full,
        identity_digest: python_identity_digest(pinned),
        selectors,
        durations_ns,
        covered_lines: Default::default(),
        complete: pinned.complete,
        generation_id: pinned.generation_id.clone(),
        raw_statuses: statuses.clone(),
        statuses,
    }
}

#[allow(dead_code)]
pub(crate) fn try_warm_python_cached_summary(
    repo_root: &Path,
    planned_selectors: &[String],
    test_args: &[String],
) -> Option<SelectorExecutionSummary> {
    let pinned = try_load_pinned_python_generation_warm(repo_root).ok()?;
    if !identity_matches_current(repo_root, &pinned.plan.base_identity, test_args) {
        return None;
    }
    let mut witness = python_witness_from_pinned(&pinned);
    let gate = GateConfig::load_for_repo(repo_root);
    if witness.raw_statuses.len() != witness.statuses.len() {
        witness.raw_statuses = witness.statuses.clone();
    }
    witness.statuses = reclassify_statuses_with_gate(
        &witness.selectors,
        &witness.raw_statuses,
        &witness.durations_ns,
        &gate,
    );
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    let mode = if planned == witness.selectors {
        AcceptMode::All
    } else {
        AcceptMode::Subset
    };
    let identity = witness.identity_digest.clone();
    if accept_witness(mode, &planned, &identity, &witness) != AcceptDecision::Accept {
        return None;
    }
    Some(summary_from_accepted_witness(
        &planned,
        &witness,
        |selector| selector.to_string(),
    ))
}
