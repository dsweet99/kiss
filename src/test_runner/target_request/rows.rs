#![cfg_attr(not(test), allow(dead_code))]
use std::collections::BTreeMap;
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};
use crate::test_runner::language_keyed::LanguageKeyed;

use super::report::{EffectiveStatus, SelectorRow};
use super::scope::ReportScope;

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

pub(crate) fn prior_failure_selectors(scope: &ReportScope, rows: &[SelectorRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| have_member(scope, &row.selector))
        .filter(|row| {
            matches!(
                row.effective,
                EffectiveStatus::Fail | EffectiveStatus::Timeout
            )
        })
        .map(|row| row.selector.clone())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::super::resolved::SourceRegion;
    use super::*;

    fn row(selector: &str, effective: EffectiveStatus) -> SelectorRow {
        SelectorRow {
            language: kiss::Language::Python,
            selector: selector.into(),
            raw: String::new(),
            effective,
            duration_ns: None,
            provenance: "test".into(),
        }
    }

    #[test]
    fn prior_failures_keep_fail_and_timeout_inside_scope() {
        let scope = ReportScope::from_membership(
            vec![SourceRegion::WorkspaceAll],
            vec!["a".into(), "b".into(), "c".into()],
            true,
        );
        let rows = vec![
            row("a", EffectiveStatus::Fail),
            row("b", EffectiveStatus::Timeout),
            row("c", EffectiveStatus::Pass),
            row("d", EffectiveStatus::Fail),
        ];
        assert_eq!(
            prior_failure_selectors(&scope, &rows),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
