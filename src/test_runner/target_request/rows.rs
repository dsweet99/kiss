#![cfg_attr(not(test), allow(dead_code))]
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};
use crate::test_runner::language_keyed::LanguageKeyed;

use super::report::{EffectiveStatus, SelectorRow};
use super::scope::{ExecutionPlan, ReportScope};

pub(crate) fn available_rows(
    repo_root: &Path,
    scope: &ReportScope,
    extras: LanguageKeyed<&[String]>,
) -> Vec<SelectorRow> {
    if scope.selectors.is_empty() {
        return Vec::new();
    }
    let by_selector = witness_map(repo_root, extras);
    scope
        .selectors
        .iter()
        .filter_map(|selector| by_selector.get(selector).cloned())
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct AvailableRowPlan {
    pub retry_bad: bool,
    pub graph_repair: bool,
    pub force: bool,
    pub time_gate_active: bool,
}

pub(crate) fn plan_from_available_rows(
    scope: &ReportScope,
    rows: &[SelectorRow],
    plan: AvailableRowPlan,
) -> ExecutionPlan {
    let AvailableRowPlan {
        retry_bad,
        graph_repair,
        force,
        time_gate_active,
    } = plan;
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

fn have_member(scope: &ReportScope, selector: &str) -> bool {
    scope.selectors.iter().any(|item| item == selector)
}

fn witness_map(
    repo_root: &Path,
    extras: LanguageKeyed<&[String]>,
) -> BTreeMap<String, SelectorRow> {
    let mut by_selector = BTreeMap::new();
    for language in kiss::Language::ALL {
        extend_witness(
            &mut by_selector,
            crate::test_runner::lang_registry::rules_for(language)
                .stored_witness(repo_root, extras.get(language)),
        );
    }
    by_selector
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
                language: witness.language,
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
