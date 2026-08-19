
use std::path::Path;

use super::identity::population_plan_for_selectors;
use super::evidence::{PopulationEvidence, SelectorEvidence, status_label};
use super::load::{GenerationLoadError, try_load_pinned_python_generation_without_line_index};
use super::publish::{
    publish_python_population_generation, publish_python_population_generation_reusing,
};
use super::types::{
    GenerationReason, PinnedPythonGeneration, PythonExecutionIdentity, PythonPopulationPlan,
    SelectorTimingRecord,
};

pub(crate) fn repair_python_population_generation(
    repo_root: &Path,
    deltas: &[SelectorEvidence],
    reason: GenerationReason,
) -> Result<Option<String>, String> {
    let pinned = match try_load_pinned_python_generation_without_line_index(repo_root) {
        Ok(pinned) => pinned,
        Err(GenerationLoadError::MissingOrStale) => {
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
    let mut pinned = pinned;
    let plan = pinned.plan.clone();
    let mut evidence = evidence_from_pinned(&plan, &mut pinned);
    for delta in changed {
        evidence.absorb_selector(delta);
    }
    let id = publish_python_population_generation(repo_root, &pinned.plan, &evidence, reason)?;
    Ok(Some(id))
}

pub(crate) fn restamp_and_repair_python_population_generation(
    repo_root: &Path,
    test_args: &[String],
    deltas: &[SelectorEvidence],
    reason: GenerationReason,
) -> Result<Option<String>, String> {
    let mut pinned = match try_load_pinned_python_generation_without_line_index(repo_root) {
        Ok(pinned) => pinned,
        Err(GenerationLoadError::MissingOrStale) => {
            return Ok(None);
        }
        Err(GenerationLoadError::Corrupt(msg)) => {
            return Err(format!("error: kiss: corrupt Python generation: {msg}"));
        }
    };
    let new_plan = population_plan_for_selectors(repo_root, &pinned.plan.selectors, test_args)?;
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
    if changed.is_empty() && pinned.plan.base_identity == new_plan.base_identity {
        return Ok(None);
    }
    let reuse_id = pinned.generation_id.clone();
    let reuse_unchanged = changed.is_empty();
    let mut evidence = evidence_from_pinned(&new_plan, &mut pinned);
    for delta in changed {
        evidence.absorb_selector(delta);
    }
    let id = if reuse_unchanged {
        publish_python_population_generation_reusing(
            repo_root,
            &new_plan,
            &evidence,
            reason,
            Some(reuse_id.as_str()),
        )?
    } else {
        publish_python_population_generation(repo_root, &new_plan, &evidence, reason)?
    };
    Ok(Some(id))
}

pub(crate) fn try_restamp_matching_pinned_universe(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    run_misses: Option<&[String]>,
) -> Result<bool, String> {
    let pinned = match try_load_pinned_python_generation_without_line_index(repo_root) {
        Ok(pinned) => pinned,
        Err(GenerationLoadError::MissingOrStale) => {
            return Ok(false);
        }
        Err(GenerationLoadError::Corrupt(msg)) => {
            return Err(format!("error: kiss: corrupt Python generation: {msg}"));
        }
    };
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    if pinned.plan.selectors != expected {
        return Ok(false);
    }
    let current = super::identity::current_python_execution_identity(repo_root, test_args)?;
    if !restamp_is_safe(
        &pinned.plan.base_identity,
        &current,
        expected.len(),
        run_misses,
    ) {
        return Ok(false);
    }
    let problems = problem_and_run_miss_selectors(&pinned.timings, run_misses);
    let deltas = super::materialize::selector_deltas_from_cached_outcomes(
        repo_root,
        &problems,
        test_args,
        is_indexable,
        &kiss::GateConfig::load_for_repo(repo_root),
    )?;
    let reason = if pinned.complete {
        GenerationReason::Complete
    } else {
        GenerationReason::IncompleteRepair
    };
    let _ = restamp_and_repair_python_population_generation(repo_root, test_args, &deltas, reason)?;
    Ok(true)
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

fn restamp_is_safe(
    pinned: &PythonExecutionIdentity,
    current: &PythonExecutionIdentity,
    selector_count: usize,
    run_misses: Option<&[String]>,
) -> bool {
    let mut comparable = current.clone();
    comparable.kissconfig_test_digest = pinned.kissconfig_test_digest.clone();
    if pinned == &comparable {
        return true;
    }
    run_misses.is_some_and(|misses| misses.len() < selector_count)
}

fn problem_and_run_miss_selectors(
    timings: &[SelectorTimingRecord],
    run_misses: Option<&[String]>,
) -> Vec<String> {
    let mut problems = problem_selectors_from_timings(timings);
    let Some(misses) = run_misses else {
        return problems;
    };
    for miss in misses {
        if !problems.iter().any(|selector| selector == miss) {
            problems.push(miss.clone());
        }
    }
    problems
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
    pinned: &mut PinnedPythonGeneration,
) -> PopulationEvidence {
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.coverage = std::mem::take(&mut pinned.coverage);
    evidence.selector_coverage = std::mem::take(&mut pinned.selector_coverage);
    evidence.timings = std::mem::take(&mut pinned.timings);
    evidence.complete = pinned.complete;
    evidence.rebuild_line_index();
    evidence
}
