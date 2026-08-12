//! Repair an existing generation with changed-selector evidence.

use std::path::Path;

use super::evidence::{PopulationEvidence, SelectorEvidence, status_label};
use super::load::{GenerationLoadError, try_load_pinned_python_generation};
use super::publish::publish_python_population_generation;
use super::types::{
    GenerationReason, PinnedPythonGeneration, PythonPopulationPlan, SelectorTimingRecord,
};

pub(crate) fn repair_python_population_generation(
    repo_root: &Path,
    deltas: &[SelectorEvidence],
    reason: GenerationReason,
) -> Result<Option<String>, String> {
    let pinned = match try_load_pinned_python_generation(repo_root) {
        Ok(pinned) => pinned,
        Err(GenerationLoadError::MissingOrStale) => {
            // Cold selective runs store outcomes in the rslip cache only; there is
            // no population generation to delta-repair until a complete publish.
            return Ok(None);
        }
        Err(GenerationLoadError::Corrupt(msg)) => {
            return Err(format!("error: kiss: corrupt Python generation: {msg}"));
        }
    };
    for delta in deltas {
        if !pinned.plan.selectors.iter().any(|s| s == &delta.selector) {
            return Err(format!(
                "error: kiss: selector `{}` is outside the current Python population",
                delta.selector
            ));
        }
    }
    let changed: Vec<SelectorEvidence> = deltas
        .iter()
        .filter(|delta| !evidence_matches_pinned(&pinned, delta))
        .cloned()
        .collect();
    if changed.is_empty() {
        return Ok(None);
    }
    let mut evidence = evidence_from_pinned(&pinned.plan, &pinned);
    for delta in changed {
        evidence.absorb_selector(delta);
    }
    let id = publish_python_population_generation(repo_root, &pinned.plan, &evidence, reason)?;
    Ok(Some(id))
}

pub(crate) fn problem_selectors_from_timings(timings: &[SelectorTimingRecord]) -> Vec<String> {
    timings
        .iter()
        .filter(|row| is_problem_status(&row.effective_status))
        .map(|row| row.selector.clone())
        .collect()
}

fn is_problem_status(status: &str) -> bool {
    status != "passed"
}

fn evidence_matches_pinned(pinned: &PinnedPythonGeneration, delta: &SelectorEvidence) -> bool {
    let Some(timing) = pinned
        .timings
        .iter()
        .find(|row| row.selector == delta.selector)
    else {
        return false;
    };
    if timing.raw_status != status_label(delta.raw_status) {
        return false;
    }
    if timing.effective_status != status_label(delta.effective_status) {
        return false;
    }
    let cov = pinned
        .selector_coverage
        .get(&delta.selector)
        .cloned()
        .unwrap_or_default();
    cov == delta.coverage
}

fn evidence_from_pinned(
    plan: &PythonPopulationPlan,
    pinned: &PinnedPythonGeneration,
) -> PopulationEvidence {
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.coverage = pinned.coverage.clone();
    evidence.selector_coverage = pinned.selector_coverage.clone();
    evidence.line_index = pinned.line_index.clone();
    evidence.timings = pinned.timings.clone();
    for (file, lines) in &pinned.line_index {
        for (line, selectors) in lines {
            evidence
                .line_refs
                .insert((file.clone(), *line), selectors.len() as u32);
        }
    }
    evidence.complete = pinned.complete;
    evidence
}
