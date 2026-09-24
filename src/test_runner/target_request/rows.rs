use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};

use super::report::{EffectiveStatus, SelectorRow};
use super::scope::{ExecutionPlan, ReportScope};

pub(crate) fn rows_from_witnesses(
    repo_root: &Path,
    scope: &ReportScope,
) -> Result<Vec<SelectorRow>, String> {
    if scope.selectors.is_empty() {
        return Ok(Vec::new());
    }
    let by_selector = witness_map(repo_root);
    let mut rows = Vec::new();
    for selector in &scope.selectors {
        let Some(row) = by_selector.get(selector) else {
            return Err(format!("missing typed evidence for {selector}"));
        };
        rows.push(row.clone());
    }
    Ok(rows)
}

pub(crate) fn available_rows(repo_root: &Path, scope: &ReportScope) -> Vec<SelectorRow> {
    if scope.selectors.is_empty() {
        return Vec::new();
    }
    let by_selector = witness_map(repo_root);
    scope
        .selectors
        .iter()
        .filter_map(|selector| by_selector.get(selector).cloned())
        .collect()
}

pub(crate) fn plan_from_available_rows(
    scope: &ReportScope,
    rows: &[SelectorRow],
    retry_bad: bool,
    graph_repair: bool,
) -> ExecutionPlan {
    plan_from_available_rows_with(scope, rows, retry_bad, graph_repair, false, false)
}

pub(crate) fn plan_from_available_rows_with(
    scope: &ReportScope,
    rows: &[SelectorRow],
    retry_bad: bool,
    graph_repair: bool,
    force: bool,
    time_gate_active: bool,
) -> ExecutionPlan {
    let have: BTreeSet<&str> = rows.iter().map(|row| row.selector.as_str()).collect();
    let mut repair_selectors: Vec<String> = scope
        .selectors
        .iter()
        .filter(|selector| !have.contains(selector.as_str()))
        .cloned()
        .collect();
    if time_gate_active {
        for row in rows {
            if have_member(scope, &row.selector)
                && row.duration_ns.is_none()
                && matches!(row.effective, EffectiveStatus::Pass | EffectiveStatus::Fail)
            {
                repair_selectors.push(row.selector.clone());
            }
        }
        repair_selectors.sort();
        repair_selectors.dedup();
    }
    let retry = if retry_bad {
        rows.iter()
            .filter(|row| have_member(scope, &row.selector))
            .filter(|row| {
                matches!(
                    row.effective,
                    EffectiveStatus::Fail | EffectiveStatus::Timeout
                ) || (time_gate_active
                    && row.duration_ns.is_none()
                    && row.effective == EffectiveStatus::Fail)
            })
            .map(|row| row.selector.clone())
            .collect()
    } else {
        Vec::new()
    };
    let forced = if force {
        scope.selectors.clone()
    } else {
        Vec::new()
    };
    ExecutionPlan {
        repair_selectors,
        retry_bad: retry,
        forced,
        population_repair: !scope.complete,
        graph_repair,
    }
}

pub(crate) fn duration_evidence_holds(
    rows: &[SelectorRow],
    time_gate_active: bool,
) -> Result<(), String> {
    if !time_gate_active {
        return Ok(());
    }
    for row in rows {
        if row.duration_ns.is_none()
            && matches!(row.effective, EffectiveStatus::Pass | EffectiveStatus::Fail)
        {
            return Err(format!("missing duration for {}", row.selector));
        }
    }
    Ok(())
}

fn have_member(scope: &ReportScope, selector: &str) -> bool {
    scope.selectors.iter().any(|item| item == selector)
}

fn witness_map(repo_root: &Path) -> BTreeMap<String, SelectorRow> {
    let mut by_selector = BTreeMap::new();
    extend_witness(&mut by_selector, python_witness(repo_root));
    extend_witness(&mut by_selector, rust_witness(repo_root));
    by_selector
}

fn python_witness(repo_root: &Path) -> Option<ExecutionWitness> {
    let pinned = crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(
        repo_root,
    )
    .ok()?;
    Some(crate::test_runner::lang_python::python_witness_from_pinned(
        &pinned,
    ))
}

fn rust_witness(repo_root: &Path) -> Option<ExecutionWitness> {
    crate::test_runner::lang_rust::try_load_rust_execution_witness(repo_root).ok()
}

fn extend_witness(out: &mut BTreeMap<String, SelectorRow>, witness: Option<ExecutionWitness>) {
    let Some(witness) = witness else {
        return;
    };
    for (i, selector) in witness.selectors.iter().enumerate() {
        let Some(raw) = witness.raw_statuses.get(i).copied() else {
            continue;
        };
        let Some(effective) = effective_of(raw) else {
            continue;
        };
        out.insert(
            selector.clone(),
            SelectorRow {
                language: witness.language.clone(),
                selector: selector.clone(),
                raw: raw.as_str().to_string(),
                effective,
                duration_ns: witness.durations_ns.get(i).copied().flatten(),
                provenance: "witness".into(),
            },
        );
    }
}

fn effective_of(raw: WitnessStatus) -> Option<EffectiveStatus> {
    match raw {
        WitnessStatus::Passed => Some(EffectiveStatus::Pass),
        WitnessStatus::Failed => Some(EffectiveStatus::Fail),
        WitnessStatus::TimedOut => Some(EffectiveStatus::Timeout),
        WitnessStatus::Unresolved => None,
    }
}
