use std::path::Path;

use rpytest_runner::TestStatus;
use rslip::RslipOutcome;

use super::evidence::{PopulationEvidence, SelectorEvidence, selector_evidence_from_outcome};
use super::identity::population_plan_for_selectors;
use super::publish::publish_python_population_generation;
use super::types::{GenerationReason, PythonPopulationPlan};
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};

pub(crate) fn materialize_and_publish_from_cached_outcomes(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
    reason: GenerationReason,
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> Result<(PythonPopulationPlan, String), String> {
    let plan = population_plan_for_selectors(repo_root, selectors, test_args)?;
    let evidence = evidence_from_cached_outcomes(repo_root, &plan, test_args, is_indexable)?;
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
    let outcomes = rslip::load_cached_outcomes_many_trusting_population(&reqs);
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

pub(crate) fn evidence_from_cached_outcomes(
    repo_root: &Path,
    plan: &PythonPopulationPlan,
    test_args: &[String],
    is_indexable: &dyn Fn(&Path, &Path) -> bool,
) -> Result<PopulationEvidence, String> {
    let deltas = selector_deltas_from_cached_outcomes(
        repo_root,
        &plan.selectors,
        test_args,
        is_indexable,
        &kiss::GateConfig::load_for_repo(repo_root),
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
    let effective = effective_status(&outcome.status, gate, selector);
    let reason = (effective != outcome.status)
        .then(|| "effective status changed by current test policy".to_string());
    selector_evidence_from_outcome(repo_root, outcome, effective, reason, is_indexable)
}

fn effective_status(raw: &TestStatus, gate: &kiss::GateConfig, selector: &str) -> TestStatus {
    if *raw != TestStatus::Passed {
        return *raw;
    }
    let _ = (gate, selector);
    TestStatus::Passed
}
