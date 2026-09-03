use std::path::Path;

use super::evidence::{PopulationEvidence, SelectorEvidence, status_label};
use super::identity::population_plan_for_selectors;
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
        .filter(|delta| !evidence_matches_pinned(repo_root, &pinned, delta))
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
        .filter(|delta| !evidence_matches_pinned(repo_root, &pinned, delta))
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
    gate: &kiss::GateConfig,
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
    if !restamp_is_safe(repo_root, &pinned, &current, run_misses) {
        return Ok(false);
    }
    let refresh = selectors_to_refresh(&pinned, run_misses);
    let deltas = super::materialize::selector_deltas_from_cached_outcomes(
        repo_root,
        &refresh,
        test_args,
        is_indexable,
        gate,
    )?;
    if deltas.len() != refresh.len() {
        return Ok(false);
    }
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
    repo_root: &Path,
    pinned: &PinnedPythonGeneration,
    current: &PythonExecutionIdentity,
    run_misses: Option<&[String]>,
) -> bool {
    if pinned.plan.base_identity != *current {
        return false;
    }
    pinned.timings.iter().all(|row| {
        row.test_definition_digest
            == crate::test_runner::python_coverage_index::storage::python_selector_definition_digest(
                repo_root,
                &row.selector,
            )
            || run_misses.is_some_and(|misses| misses.contains(&row.selector))
    })
}

pub(crate) fn restamp_complete_pinned_from_cache(
    repo_root: &Path,
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    gate: &kiss::GateConfig,
) -> Result<bool, String> {
    let Ok(pinned) = try_load_pinned_python_generation_without_line_index(repo_root) else {
        return Ok(false);
    };
    if !pinned.complete {
        return Ok(false);
    }
    try_restamp_matching_pinned_universe(
        repo_root,
        &pinned.plan.selectors,
        test_args,
        is_indexable,
        gate,
        None,
    )
}

fn selectors_to_refresh(
    pinned: &PinnedPythonGeneration,
    run_misses: Option<&[String]>,
) -> Vec<String> {
    if pinned.complete {
        run_misses.unwrap_or_default().to_vec()
    } else {
        problem_and_run_miss_selectors(&pinned.timings, run_misses)
    }
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

fn evidence_matches_pinned(
    repo_root: &Path,
    pinned: &PinnedPythonGeneration,
    delta: &SelectorEvidence,
) -> bool {
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
    if timing.test_definition_digest
        != crate::test_runner::python_coverage_index::storage::python_selector_definition_digest(
            repo_root,
            &delta.selector,
        )
    {
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

#[cfg(test)]
mod refresh_tests {
    use super::super::types::{
        PythonExecutionIdentity, PythonPopulationPlan, TimingCacheDisposition,
    };
    use super::{
        PinnedPythonGeneration, SelectorTimingRecord, restamp_complete_pinned_from_cache,
        restamp_is_safe, selectors_to_refresh,
    };

    fn pin(
        complete: bool,
        selectors: &[&str],
        timings: Vec<SelectorTimingRecord>,
    ) -> PinnedPythonGeneration {
        PinnedPythonGeneration {
            generation_id: "g".into(),
            plan: PythonPopulationPlan {
                base_identity: PythonExecutionIdentity {
                    schema_version: String::new(),
                    runner_semantics_version: String::new(),
                    collector_semantics_version: String::new(),
                    source_root: String::new(),
                    interpreter_identity: String::new(),
                    python_version: String::new(),
                    pytest_version: String::new(),
                    plugin_identities: Vec::new(),
                    pytest_args: Vec::new(),
                    pytest_config_digest: String::new(),
                    kissconfig_test_digest: String::new(),
                    coverage_env_digest: String::new(),
                    env: std::collections::BTreeMap::new(),
                    input_fingerprint: String::new(),
                    selector_discovery_version: String::new(),
                    cache_schema_version: String::new(),
                },
                selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
            },
            complete,
            coverage: Default::default(),
            timings,
            line_index: Default::default(),
            selector_coverage: Default::default(),
        }
    }

    fn timing(selector: &str, status: &str) -> SelectorTimingRecord {
        SelectorTimingRecord {
            selector: selector.to_string(),
            raw_status: status.to_string(),
            effective_status: status.to_string(),
            duration_ns: None,
            cache_disposition: TimingCacheDisposition::Hit,
            reason: None,
            test_definition_digest: String::new(),
        }
    }

    #[test]
    fn complete_generation_refreshes_only_run_misses() {
        let pinned = pin(
            true,
            &["a::t", "b::t"],
            vec![timing("a::t", "passed"), timing("b::t", "failed")],
        );
        assert_eq!(
            selectors_to_refresh(&pinned, Some(&["a::t".into()])),
            vec!["a::t".to_string()]
        );
        assert!(selectors_to_refresh(&pinned, None).is_empty());
    }

    #[test]
    fn incomplete_generation_refreshes_problems_and_run_misses() {
        let pinned = pin(
            false,
            &["a::t", "b::t", "c::t"],
            vec![
                timing("a::t", "passed"),
                timing("b::t", "failed"),
                timing("c::t", "passed"),
            ],
        );
        assert_eq!(
            selectors_to_refresh(&pinned, Some(&["c::t".into()])),
            vec!["b::t".to_string(), "c::t".to_string()]
        );
    }

    #[test]
    fn missing_pin_does_not_restamp_complete_generation() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            !restamp_complete_pinned_from_cache(
                tmp.path(),
                &[],
                &|_, _| true,
                &kiss::GateConfig::default(),
            )
            .unwrap()
        );
    }

    #[test]
    fn restamp_requires_changed_test_to_be_a_run_miss() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("t.py"), "def test_a():\n    pass\n").unwrap();
        let mut pinned = pin(
            true,
            &["t.py::test_a"],
            vec![timing("t.py::test_a", "passed")],
        );
        pinned.timings[0].test_definition_digest = "stale".into();
        let current = pinned.plan.base_identity.clone();
        assert!(!restamp_is_safe(tmp.path(), &pinned, &current, None));
        assert!(restamp_is_safe(
            tmp.path(),
            &pinned,
            &current,
            Some(&["t.py::test_a".into()])
        ));
    }
}
