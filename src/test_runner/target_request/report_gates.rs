#![cfg_attr(not(test), allow(dead_code))]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::report::{ReportGate, SelectorRow, configuration_generation};
use super::resolved::SourceRegion;
use super::scope::ReportScope;

pub(super) fn workspace_source_files(repo_root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let root = repo_root.to_string_lossy().into_owned();
    let (mut py, mut rs) = kiss::gather_files_by_lang(std::slice::from_ref(&root), None, &[]);
    py.sort();
    py.dedup();
    rs.sort();
    rs.dedup();
    (py, rs)
}

pub(super) fn scope_has_orphan_candidates(repo_root: &Path, scope: &ReportScope) -> bool {
    let (py, rs) = production_files_for_scope(repo_root, scope);
    !py.is_empty() || !rs.is_empty()
}

pub(super) fn graph_evidence_parts(
    repo_root: &Path,
    gate: &kiss::GateConfig,
) -> (String, Vec<PathBuf>, Vec<PathBuf>) {
    let (py, rs) = workspace_source_files(repo_root);
    let key = super::graph_store::evidence_key(
        repo_root,
        &py,
        &rs,
        &gate.orphan_allowed,
        &configuration_generation(repo_root),
    );
    (key, py, rs)
}

pub(super) fn production_files_for_scope(
    repo_root: &Path,
    scope: &ReportScope,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut py = Vec::new();
    let mut rs = Vec::new();
    let workspace = scope
        .regions
        .iter()
        .any(|region| matches!(region, SourceRegion::WorkspaceAll));
    if workspace {
        let root = repo_root.to_string_lossy().into_owned();
        let (gathered_py, gathered_rs) =
            kiss::gather_files_by_lang(std::slice::from_ref(&root), None, &[]);
        py.extend(
            gathered_py
                .into_iter()
                .filter(|path| production_source_abs(path)),
        );
        rs.extend(
            gathered_rs
                .into_iter()
                .filter(|path| production_source_abs(path)),
        );
    }
    for region in &scope.regions {
        let rel = match region {
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => path,
            SourceRegion::WorkspaceAll => continue,
        };
        if !production_source_path(rel) {
            continue;
        }
        let abs = repo_root.join(rel);
        if !abs.is_file() {
            continue;
        }
        if rel.ends_with(".py") {
            py.push(abs);
        } else if rel.ends_with(".rs") {
            rs.push(abs);
        }
    }
    py.sort();
    py.dedup();
    rs.sort();
    rs.dedup();
    (py, rs)
}

pub(super) fn production_source_path(path: &str) -> bool {
    !path.contains("::") && production_source_abs(Path::new(path))
}

pub(super) fn production_source_abs(path: &Path) -> bool {
    !crate::test_runner::lang_registry::all_rules().any(|rules| rules.is_test_source(path))
}

pub(super) fn focused_line_set(scope: &ReportScope, file: &str) -> Option<BTreeSet<u32>> {
    if scope.regions.iter().any(|region| match region {
        SourceRegion::WorkspaceAll => true,
        SourceRegion::FileAll { path } => file == path || file.starts_with(&format!("{path}/")),
        SourceRegion::FileLines { .. } => false,
    }) {
        return None;
    }
    let mut lines = BTreeSet::new();
    let mut found = false;
    for region in &scope.regions {
        if let SourceRegion::FileLines { path, lines: focus } = region
            && (file == path || file.starts_with(&format!("{path}/")))
        {
            found = true;
            lines.extend(focus.iter().copied());
        }
    }
    found.then_some(lines)
}

pub(super) fn population_selector_count(
    repo_root: &Path,
    request: &super::types::TargetRequest,
    need: crate::test_runner::workspace_selector_cache::SelectorCountNeed,
) -> Option<usize> {
    if let Some((first, second)) =
        crate::test_runner::workspace_selector_cache::load_workspace_selectors_for_count(
            repo_root,
            &request.ignore,
            &[],
            need,
        )
    {
        return Some(first.len() + second.len());
    }
    let mut count = 0;
    for language in crate::test_runner::lang_registry::languages()
        .into_iter()
        .filter(|language| need.wants(*language))
    {
        count += crate::test_runner::lang_registry::rules_for(language)
            .list_workspace_selectors(repo_root, &request.ignore, &[])
            .ok()?
            .len();
    }
    Some(count)
}

pub(super) fn gates_from_population(
    repo_root: &Path,
    gate: &kiss::GateConfig,
    request: &super::types::TargetRequest,
) -> Vec<ReportGate> {
    let need = super::manifest::population_count_need(repo_root, request);
    let Some(count) = population_selector_count(repo_root, request, need) else {
        return vec![ReportGate {
            kind: "max_num_tests".into(),
            detail: "population evidence incomplete".into(),
        }];
    };
    if count > gate.max_num_tests {
        vec![ReportGate {
            kind: "max_num_tests".into(),
            detail: format!(
                "{count} test(s) exceeds max_num_tests={}",
                gate.max_num_tests
            ),
        }]
    } else {
        Vec::new()
    }
}

pub(super) fn gates_from_orphan(
    repo_root: &Path,
    gate: &kiss::GateConfig,
    scope: &ReportScope,
) -> Vec<ReportGate> {
    if !gate.orphan_detection || !scope_has_orphan_candidates(repo_root, scope) {
        return Vec::new();
    }
    let (key, py, rs) = graph_evidence_parts(repo_root, gate);
    if py.is_empty() && rs.is_empty() {
        return Vec::new();
    }
    let Some(items) = super::graph_store::load_items(repo_root, &key) else {
        return vec![ReportGate {
            kind: "orphan".into(),
            detail: "graph evidence incomplete".into(),
        }];
    };
    items
        .into_iter()
        .filter(|item| scope_includes_orphan(scope, item))
        .map(|item| ReportGate {
            kind: "orphan".into(),
            detail: format!("{}:{}", item.file, item.unit_name),
        })
        .collect()
}

pub(super) fn gates_from_timing(rows: &[SelectorRow], gate: &kiss::GateConfig) -> Vec<ReportGate> {
    if gate.max_unit_test_seconds.is_empty() {
        return Vec::new();
    }
    rows.iter()
        .filter_map(|row| {
            let seconds = std::time::Duration::from_nanos(row.duration_ns?).as_secs_f64();
            kiss::exceeds_limit(&gate.max_unit_test_seconds, &row.selector, seconds).then(|| {
                ReportGate {
                    kind: "max_unit_test_seconds".into(),
                    detail: row.selector.clone(),
                }
            })
        })
        .collect()
}

pub(super) fn scope_includes_orphan(
    scope: &ReportScope,
    item: &super::graph_store::GraphOrphanItem,
) -> bool {
    if !scope_includes_file(scope, &item.file) {
        return false;
    }
    match focused_line_set(scope, &item.file) {
        None => true,
        Some(focus) => unit_overlaps_focus(item.start_line, item.end_line, &focus),
    }
}

pub(super) fn unit_overlaps_focus(start: u32, end: u32, focus: &BTreeSet<u32>) -> bool {
    if start == 0 && end == 0 {
        return true;
    }
    let lo = start.max(1);
    let hi = end.max(lo);
    (lo..=hi).any(|line| focus.contains(&line))
}

pub(super) fn scope_includes_file(scope: &ReportScope, file: &str) -> bool {
    if scope.regions.is_empty()
        || scope
            .regions
            .iter()
            .any(|region| matches!(region, SourceRegion::WorkspaceAll))
    {
        return true;
    }
    scope.regions.iter().any(|region| match region {
        SourceRegion::WorkspaceAll => true,
        SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
            file == path || file.starts_with(&format!("{path}/"))
        }
    })
}
