use std::path::Path;

use kiss::rpytest_runner::TestStatus;
use kiss::rslip::RslipOutcome;

use super::evidence::{PopulationEvidence, SelectorEvidence, selector_evidence_from_outcome};
use super::identity::population_plan_for_selectors;
use super::publish::publish_python_population_generation;
use super::types::{GenerationReason, PythonPopulationPlan, TimingCacheDisposition};
use crate::test_runner::runners::{
    SelectorExecutionSummary, detect_rslip_versions, rslip_request_from_parts,
};

pub(crate) fn materialize_and_publish_from_cached_outcomes(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    reason: GenerationReason,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    gate: &kiss::GateConfig,
) -> Result<(PythonPopulationPlan, String), String> {
    let plan = population_plan_for_selectors(repo_root, selectors, test_args)?;
    let evidence = evidence_from_cached_outcomes(repo_root, &plan, test_args, is_indexable, gate)?;
    let generation_id = publish_python_population_generation(repo_root, &plan, &evidence, reason)?;
    Ok((plan, generation_id))
}

pub(crate) fn selector_deltas_from_cached_outcomes(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    gate: &kiss::GateConfig,
) -> Result<Vec<SelectorEvidence>, String> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let (python_version, pytest_version) = detect_rslip_versions(repo_root)?;
    let reqs = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                test_args,
                &python_version,
                &pytest_version,
                false,
                gate,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let outcomes = kiss::rslip::load_cached_outcomes_many(&reqs);
    let mut deltas = Vec::new();
    for (selector, outcome) in selectors.iter().zip(outcomes) {
        let Some(outcome) =
            outcome.map_err(|e| format!("error: kiss: malformed Python cache entry: {e:?}"))?
        else {
            continue;
        };
        if outcome.nodeid != *selector {
            continue;
        }
        deltas.push(outcome_to_evidence(
            repo_root,
            &outcome,
            gate,
            selector,
            is_indexable,
        ));
    }
    Ok(deltas)
}

pub(crate) fn selector_deltas_from_fresh_outcomes(
    repo_root: &Path,
    selectors: &[String],
    summary: &SelectorExecutionSummary,
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    gate: &kiss::GateConfig,
) -> Result<Vec<SelectorEvidence>, String> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let (python_version, pytest_version) = detect_rslip_versions(repo_root)?;
    let reqs = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                test_args,
                &python_version,
                &pytest_version,
                false,
                gate,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let outcomes = kiss::rslip::load_cached_outcomes_many_trusting_population(&reqs);
    selectors
        .iter()
        .zip(outcomes)
        .map(|(selector, outcome)| {
            let raw = summary
                .raw_statuses
                .get(selector)
                .copied()
                .ok_or_else(|| format!("error: kiss: missing fresh status for {selector}"))?;
            let duration = summary
                .selector_durations_ns
                .get(selector)
                .copied()
                .map(std::time::Duration::from_nanos);
            let effective = if summary.timed_out_selectors.contains(selector) {
                TestStatus::TimedOut
            } else if summary.failed_selectors.contains(selector) {
                TestStatus::Failed
            } else {
                TestStatus::Passed
            };
            if summary.cache_unstored_selectors.contains(selector) {
                return Ok(SelectorEvidence {
                    selector: selector.clone(),
                    raw_status: raw,
                    effective_status: effective,
                    duration,
                    cache_disposition: TimingCacheDisposition::MissUnstored,
                    reason: (raw != effective)
                        .then(|| "effective status changed by current test policy".to_string()),
                    coverage: Default::default(),
                });
            }
            let outcome = outcome
                .map_err(|e| format!("error: kiss: malformed fresh Python cache entry: {e:?}"))?
                .filter(|outcome| {
                    outcome.nodeid == *selector
                        && outcome.status == raw
                        && Some(outcome.duration) == duration
                })
                .ok_or_else(|| {
                    format!("error: kiss: missing fresh Python cache entry for {selector}")
                })?;
            let mut evidence =
                outcome_to_evidence(repo_root, &outcome, gate, selector, is_indexable);
            evidence.cache_disposition = TimingCacheDisposition::MissStored;
            evidence.effective_status = effective;
            evidence.reason = (raw != effective)
                .then(|| "effective status changed by current test policy".to_string());
            Ok(evidence)
        })
        .collect()
}

pub(crate) fn evidence_from_cached_outcomes(
    repo_root: &Path,
    plan: &PythonPopulationPlan,
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
    gate: &kiss::GateConfig,
) -> Result<PopulationEvidence, String> {
    let deltas = selector_deltas_from_cached_outcomes(
        repo_root,
        &plan.selectors,
        test_args,
        is_indexable,
        gate,
    )?;
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    for delta in deltas {
        evidence.absorb_selector(delta);
    }

    evidence.recompute_complete();
    Ok(evidence)
}

fn outcome_to_evidence(
    repo_root: &Path,
    outcome: &RslipOutcome,
    gate: &kiss::GateConfig,
    selector: &str,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> SelectorEvidence {
    let effective = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        selector,
        outcome.duration,
        gate,
    );
    let reason = (effective != outcome.status)
        .then(|| "effective status changed by current test policy".to_string());
    selector_evidence_from_outcome(repo_root, outcome, effective, reason, is_indexable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn stored_outcome_evidence_applies_time_gate() {
        let tmp = tempfile::tempdir().unwrap();
        let selector = "tests/test_a.py::test_a";
        let outcome = RslipOutcome {
            nodeid: selector.into(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_secs(2),
            coverage: kiss::rslip::LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: kiss::rslip::CacheStatus::MissStored,
            stdout: None,
            stderr: None,
        };
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 1.0)],
            ..Default::default()
        };
        let evidence = outcome_to_evidence(tmp.path(), &outcome, &gate, selector, &|_, _| true);
        assert_eq!(evidence.raw_status, TestStatus::Passed);
        assert_eq!(evidence.effective_status, TestStatus::TimedOut);
    }
}
