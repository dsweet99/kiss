use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::resolved::SourceRegion;
use super::scope::{ExecutionPlan, ReportScope};
use super::slice::TargetSliceStamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum EffectiveStatus {
    Pass,
    Fail,
    Timeout,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectorRow {
    pub language: String,
    pub selector: String,
    pub raw: String,
    pub effective: EffectiveStatus,
    pub duration_ns: Option<u64>,
    pub provenance: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportEvidenceStamp {
    pub digest: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportSnapshot {
    #[serde(default)]
    pub slice: TargetSliceStamp,
    #[serde(default)]
    pub evidence: ReportEvidenceStamp,
    #[serde(default)]
    pub graph_generation: Option<String>,
    #[serde(default)]
    pub worktree: String,
    #[serde(default)]
    pub gate_policy: String,
    #[serde(default)]
    pub runner: String,
    #[serde(default)]
    pub python_witness: Option<String>,
    #[serde(default)]
    pub python_coverage: Option<String>,
    #[serde(default)]
    pub rust_witness: Option<String>,
    #[serde(default)]
    pub rust_coverage: Option<String>,
    #[serde(default)]
    pub resolved: String,
    #[serde(default)]
    pub population: Option<String>,
    #[serde(default)]
    pub configuration: String,
    #[serde(default)]
    pub extra: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportFileCoverage {
    pub path: String,
    pub covered: u64,
    pub coverable: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportCoverage {
    pub covered: u64,
    pub coverable: u64,
    #[serde(default)]
    pub uncovered: Vec<String>,
    #[serde(default)]
    pub files: Vec<ReportFileCoverage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReportGate {
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetReport {
    pub scope: ReportScope,
    pub rows: Vec<SelectorRow>,
    pub stamp: TargetSliceStamp,
    pub exit_code: i32,
    pub evidence: ReportEvidenceStamp,
    #[serde(default)]
    pub coverage: ReportCoverage,
    #[serde(default)]
    pub gates: Vec<ReportGate>,
    #[serde(default)]
    pub coverage_all: bool,
    #[serde(default)]
    pub graph_generation: Option<String>,
    #[serde(default)]
    pub snapshot: ReportSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetPlanPreview {
    pub scope: ReportScope,
    pub plan: ExecutionPlan,
    pub membership_complete: bool,
    pub deferred: bool,
}

impl TargetReport {
    pub(crate) fn assembled_in(
        repo_root: &Path,
        request: &super::types::TargetRequest,
        scope: ReportScope,
        rows: Vec<SelectorRow>,
        stamp: TargetSliceStamp,
        exit_code: i32,
        coverage_all: bool,
    ) -> Self {
        let (lines, coverable) = coverage_maps_from_stores(repo_root, &scope);
        let coverage = coverage_from_maps(&scope, lines.clone(), coverable);
        let cfg = kiss::GateConfig::load_for_repo(repo_root);
        let mut gates = if coverage_all {
            gates_from_coverage_all(&coverage)
        } else {
            gates_from_coverage(&coverage, &cfg)
        };
        gates.extend(gates_from_timing(&rows, &cfg));
        if !coverage_all {
            gates.extend(gates_from_population(repo_root, &cfg, request));
            gates.extend(gates_from_orphan(
                repo_root,
                &cfg,
                &scope,
                &workspace_covered_from_stores(repo_root),
            ));
        }
        let graph_generation = graph_generation_id(
            repo_root,
            &scope,
            &cfg,
            coverage_all,
            &workspace_covered_from_stores(repo_root),
        );
        let population = population_inventory_id(repo_root, request);
        let mut built =
            Self::assembled_with(scope, rows, stamp, exit_code, coverage, gates, coverage_all);
        built.evidence = evidence_stamp_parts(
            &built.rows,
            &built.stamp,
            built.exit_code,
            &built.coverage,
            &built.gates,
            graph_generation.as_deref(),
            population.as_deref(),
        );
        built.graph_generation = graph_generation.clone();
        built.snapshot = snapshot_token(
            repo_root,
            request,
            &built.stamp,
            &built.evidence,
            graph_generation,
            &cfg,
            coverage_all,
        );
        built
    }

    fn assembled_with(
        scope: ReportScope,
        rows: Vec<SelectorRow>,
        stamp: TargetSliceStamp,
        exit_code: i32,
        coverage: ReportCoverage,
        gates: Vec<ReportGate>,
        coverage_all: bool,
    ) -> Self {
        let exit_code = Self::apply_gate_exit(exit_code, &gates);
        let evidence = evidence_stamp(&rows, &stamp, exit_code, &coverage, &gates, None);
        Self {
            scope,
            rows,
            stamp,
            exit_code,
            evidence,
            coverage,
            gates,
            coverage_all,
            graph_generation: None,
            snapshot: ReportSnapshot::default(),
        }
    }

    pub(crate) fn exit_for(worst: Option<EffectiveStatus>) -> i32 {
        match worst {
            Some(EffectiveStatus::Timeout) => 124,
            Some(EffectiveStatus::Fail) => 1,
            Some(EffectiveStatus::Pass) | None => 0,
        }
    }

    pub(crate) fn exit_from_rows(rows: &[SelectorRow]) -> i32 {
        Self::exit_for(rows.iter().map(|row| row.effective).reduce(worst_status))
    }

    pub(crate) fn apply_gate_exit(exit_code: i32, gates: &[ReportGate]) -> i32 {
        if gates.iter().any(|gate| {
            matches!(
                gate.kind.as_str(),
                "test_coverage" | "max_unit_test_seconds" | "max_num_tests" | "orphan"
            )
        }) {
            Self::combine_exit(exit_code, 1)
        } else {
            exit_code
        }
    }

    pub(crate) fn combine_exit(row_exit: i32, caller_exit: i32) -> i32 {
        if row_exit == 124 || caller_exit == 124 {
            124
        } else if row_exit != 0 {
            row_exit
        } else {
            caller_exit
        }
    }
}

#[cfg(test)]
fn coverage_from_stores(repo_root: &Path, scope: &ReportScope) -> ReportCoverage {
    let (lines, coverable_lines) = coverage_maps_from_stores(repo_root, scope);
    coverage_from_maps(scope, lines, coverable_lines)
}

fn coverage_maps_from_stores(
    repo_root: &Path,
    scope: &ReportScope,
) -> (
    BTreeMap<String, BTreeSet<u32>>,
    BTreeMap<String, BTreeSet<u32>>,
) {
    let mut lines: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    let mut coverable_lines: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    if let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation(repo_root)
            .or_else(|_| {
                crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(
                    repo_root,
                )
            })
    {
        for (file, covered) in pinned.coverage {
            extend_focused(scope, &file, covered, &mut lines);
        }
        for (file, indexed) in &pinned.line_index.files {
            extend_focused(scope, file, indexed.keys().copied(), &mut coverable_lines);
        }
    }
    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if let Ok((generation, _)) =
        crate::test_runner::execution_generation::load_current_generation(&cache)
    {
        for (file, covered) in generation.covered_lines {
            extend_focused(scope, &file, covered, &mut lines);
        }
    }
    extend_current_source_coverable(repo_root, scope, &mut coverable_lines);
    (lines, coverable_lines)
}

fn workspace_covered_from_stores(repo_root: &Path) -> BTreeMap<String, BTreeSet<u32>> {
    let mut lines: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    if let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation(repo_root)
            .or_else(|_| {
                crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(
                    repo_root,
                )
            })
    {
        for (file, covered) in pinned.coverage {
            lines.entry(file).or_default().extend(covered);
        }
    }
    let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if let Ok((generation, _)) =
        crate::test_runner::execution_generation::load_current_generation(&cache)
    {
        for (file, covered) in generation.covered_lines {
            lines.entry(file).or_default().extend(covered);
        }
    }
    lines
}

fn workspace_source_files(repo_root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let root = repo_root.to_string_lossy().into_owned();
    let (mut py, mut rs) = kiss::gather_files_by_lang(std::slice::from_ref(&root), None, &[]);
    py.sort();
    py.dedup();
    rs.sort();
    rs.dedup();
    (py, rs)
}

fn scope_has_orphan_candidates(repo_root: &Path, scope: &ReportScope) -> bool {
    let (py, rs) = production_files_for_scope(repo_root, scope);
    !py.is_empty() || !rs.is_empty()
}

fn graph_evidence_parts(
    repo_root: &Path,
    gate: &kiss::GateConfig,
    covered: &BTreeMap<String, BTreeSet<u32>>,
) -> (String, Vec<PathBuf>, Vec<PathBuf>) {
    let (py, rs) = workspace_source_files(repo_root);
    let key = super::graph_store::evidence_key(
        repo_root,
        &py,
        &rs,
        &gate.orphan_allowed,
        covered,
        &configuration_generation(repo_root),
    );
    (key, py, rs)
}

fn production_files_for_scope(
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
                .filter(|path| production_coverage_abs(path)),
        );
        rs.extend(
            gathered_rs
                .into_iter()
                .filter(|path| production_coverage_abs(path)),
        );
    }
    for region in &scope.regions {
        let rel = match region {
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => path,
            SourceRegion::WorkspaceAll => continue,
        };
        if !production_coverage_path(rel) {
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

fn extend_current_source_coverable(
    repo_root: &Path,
    scope: &ReportScope,
    dest: &mut BTreeMap<String, BTreeSet<u32>>,
) {
    let (py, rs) = production_files_for_scope(repo_root, scope);
    if py.is_empty() && rs.is_empty() {
        return;
    }
    let Ok(facts) = crate::analyze::line_coverage::CoverageSourceFacts::from_files(&py, &rs) else {
        return;
    };
    for (abs, lines) in facts.coverable_map() {
        let rel = abs
            .strip_prefix(repo_root)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs.to_string_lossy().into_owned());
        extend_focused(
            scope,
            &rel,
            lines.iter().filter_map(|line| u32::try_from(*line).ok()),
            dest,
        );
    }
}

fn coverage_from_maps(
    scope: &ReportScope,
    lines: BTreeMap<String, BTreeSet<u32>>,
    coverable_lines: BTreeMap<String, BTreeSet<u32>>,
) -> ReportCoverage {
    let covered = lines.values().map(|set| set.len() as u64).sum();
    let indexed: u64 = coverable_lines.values().map(|set| set.len() as u64).sum();
    let mut uncovered: Vec<String> = Vec::new();
    for region in &scope.regions {
        match region {
            SourceRegion::WorkspaceAll => {
                for (path, coverable) in &coverable_lines {
                    if production_coverage_path(path)
                        && region_has_uncovered(lines.get(path), coverable)
                    {
                        uncovered.push(path.clone());
                    }
                }
            }
            SourceRegion::FileAll { path } => {
                if !production_coverage_path(path) {
                    continue;
                }
                let coverable = coverable_lines.get(path).cloned().unwrap_or_default();
                let covered = lines.get(path);
                if coverable.is_empty() {
                    if covered.is_none_or(BTreeSet::is_empty) {
                        uncovered.push(path.clone());
                    }
                } else if region_has_uncovered(covered, &coverable) {
                    uncovered.push(path.clone());
                }
            }
            SourceRegion::FileLines { path, lines: focus } => {
                if production_coverage_path(path) && region_has_uncovered(lines.get(path), focus) {
                    uncovered.push(path.clone());
                }
            }
        }
    }
    uncovered.sort();
    uncovered.dedup();
    ReportCoverage {
        covered,
        coverable: indexed.max(covered),
        uncovered,
        files: file_coverages(scope, &lines, &coverable_lines),
    }
}

fn file_coverages(
    scope: &ReportScope,
    lines: &BTreeMap<String, BTreeSet<u32>>,
    coverable_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> Vec<ReportFileCoverage> {
    let mut files: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for region in &scope.regions {
        match region {
            SourceRegion::WorkspaceAll => {
                let mut paths: BTreeSet<&String> = BTreeSet::new();
                paths.extend(coverable_lines.keys());
                paths.extend(lines.keys());
                for path in paths {
                    if production_coverage_path(path) {
                        files.insert(
                            path.clone(),
                            file_counts(lines.get(path), coverable_lines.get(path)),
                        );
                    }
                }
            }
            SourceRegion::FileAll { path } => {
                if production_coverage_path(path) {
                    files.insert(
                        path.clone(),
                        file_counts(lines.get(path), coverable_lines.get(path)),
                    );
                }
            }
            SourceRegion::FileLines { path, lines: focus } => {
                if !production_coverage_path(path) {
                    continue;
                }
                let covered = lines.get(path).cloned().unwrap_or_default();
                let covered_n = focus.iter().filter(|line| covered.contains(line)).count() as u64;
                let coverable_n = coverable_lines.get(path).map_or(focus.len() as u64, |set| {
                    let focused = focus.intersection(set).count() as u64;
                    if focused == 0 {
                        focus.len() as u64
                    } else {
                        focused
                    }
                });
                files.insert(path.clone(), (covered_n, coverable_n.max(covered_n)));
            }
        }
    }
    files
        .into_iter()
        .map(|(path, (covered, coverable))| ReportFileCoverage {
            path,
            covered,
            coverable,
        })
        .collect()
}

fn file_counts(covered: Option<&BTreeSet<u32>>, coverable: Option<&BTreeSet<u32>>) -> (u64, u64) {
    let empty = BTreeSet::new();
    let covered = covered.unwrap_or(&empty);
    let coverable = coverable.unwrap_or(&empty);
    if coverable.is_empty() {
        let n = covered.len() as u64;
        return (n, n);
    }
    let hit = covered.intersection(coverable).count() as u64;
    let den = coverable.len() as u64;
    (hit, den.max(hit))
}

fn region_has_uncovered(covered: Option<&BTreeSet<u32>>, focus: &BTreeSet<u32>) -> bool {
    let covered = covered.cloned().unwrap_or_default();
    focus.iter().any(|line| !covered.contains(line))
}

fn production_coverage_path(path: &str) -> bool {
    !path.contains("::") && production_coverage_abs(Path::new(path))
}

fn production_coverage_abs(path: &Path) -> bool {
    !kiss::is_python_test_module_path(path)
}

fn extend_focused(
    scope: &ReportScope,
    file: &str,
    values: impl IntoIterator<Item = u32>,
    dest: &mut BTreeMap<String, BTreeSet<u32>>,
) {
    if !scope_includes_file(scope, file) {
        return;
    }
    let incoming: BTreeSet<u32> = values.into_iter().collect();
    let focused = match focused_line_set(scope, file) {
        None => incoming,
        Some(focus) => incoming.intersection(&focus).copied().collect(),
    };
    if !focused.is_empty() {
        dest.entry(file.to_string()).or_default().extend(focused);
    }
}

fn focused_line_set(scope: &ReportScope, file: &str) -> Option<BTreeSet<u32>> {
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

fn gates_from_coverage_all(coverage: &ReportCoverage) -> Vec<ReportGate> {
    coverage
        .uncovered
        .iter()
        .map(|path| ReportGate {
            kind: "test_coverage".into(),
            detail: path.clone(),
        })
        .collect()
}

fn gates_from_coverage(coverage: &ReportCoverage, gate: &kiss::GateConfig) -> Vec<ReportGate> {
    if gate.test_coverage_threshold == 0 {
        return Vec::new();
    }
    match gate.test_coverage_scope {
        kiss::TestCoverageScope::Codebase => {
            if coverage.coverable == 0 && !coverage.uncovered.is_empty() {
                return vec![ReportGate {
                    kind: "test_coverage".into(),
                    detail: format!(
                        "codebase coverage 0% below {}% threshold",
                        gate.test_coverage_threshold
                    ),
                }];
            }
            let percent = coverage_percent(coverage.covered, coverage.coverable);
            if percent >= gate.test_coverage_threshold {
                Vec::new()
            } else {
                vec![ReportGate {
                    kind: "test_coverage".into(),
                    detail: format!(
                        "codebase coverage {percent}% below {}% threshold",
                        gate.test_coverage_threshold
                    ),
                }]
            }
        }
        kiss::TestCoverageScope::ByFile => {
            if coverage.files.is_empty() {
                return coverage
                    .uncovered
                    .iter()
                    .map(|path| ReportGate {
                        kind: "test_coverage".into(),
                        detail: path.clone(),
                    })
                    .collect();
            }
            coverage
                .files
                .iter()
                .filter(|file| {
                    file_coverage_percent(file.covered, file.coverable)
                        < gate.test_coverage_threshold
                })
                .map(|file| ReportGate {
                    kind: "test_coverage".into(),
                    detail: file.path.clone(),
                })
                .collect()
        }
    }
}

fn gates_from_population(
    repo_root: &Path,
    gate: &kiss::GateConfig,
    request: &super::types::TargetRequest,
) -> Vec<ReportGate> {
    let (need_python, need_rust) = super::manifest::needed_langs(repo_root, request);
    let need = crate::test_runner::workspace_selector_cache::SelectorCountNeed {
        python: need_python && super::manifest::has_python_test_files(repo_root, request),
        rust: need_rust,
    };
    let Some((py, rs)) =
        crate::test_runner::workspace_selector_cache::load_workspace_selectors_for_count(
            repo_root,
            &request.ignore,
            &[],
            need,
        )
    else {
        return vec![ReportGate {
            kind: "max_num_tests".into(),
            detail: "population evidence incomplete".into(),
        }];
    };
    let count = py.len() + rs.len();
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

fn gates_from_orphan(
    repo_root: &Path,
    gate: &kiss::GateConfig,
    scope: &ReportScope,
    covered: &BTreeMap<String, BTreeSet<u32>>,
) -> Vec<ReportGate> {
    if !gate.orphan_detection || !scope_has_orphan_candidates(repo_root, scope) {
        return Vec::new();
    }
    let (key, py, rs) = graph_evidence_parts(repo_root, gate, covered);
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

fn gates_from_timing(rows: &[SelectorRow], gate: &kiss::GateConfig) -> Vec<ReportGate> {
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

fn coverage_percent(covered: u64, coverable: u64) -> usize {
    if coverable == 0 {
        return 100;
    }
    let covered = usize::try_from(covered).unwrap_or(usize::MAX);
    let coverable = usize::try_from(coverable).unwrap_or(usize::MAX);
    crate::analyze::line_coverage::coverage_percentage(covered, coverable)
}

fn file_coverage_percent(covered: u64, coverable: u64) -> usize {
    if coverable == 0 {
        return 0;
    }
    coverage_percent(covered, coverable)
}

fn scope_includes_orphan(scope: &ReportScope, item: &super::graph_store::GraphOrphanItem) -> bool {
    if !scope_includes_file(scope, &item.file) {
        return false;
    }
    match focused_line_set(scope, &item.file) {
        None => true,
        Some(focus) => unit_overlaps_focus(item.start_line, item.end_line, &focus),
    }
}

fn unit_overlaps_focus(start: u32, end: u32, focus: &BTreeSet<u32>) -> bool {
    if start == 0 && end == 0 {
        return true;
    }
    let lo = start.max(1);
    let hi = end.max(lo);
    (lo..=hi).any(|line| focus.contains(&line))
}

fn scope_includes_file(scope: &ReportScope, file: &str) -> bool {
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

pub(crate) fn repair_graph_evidence(
    repo_root: &Path,
    scope: &ReportScope,
    coverage_all: bool,
) -> Result<(), String> {
    if !graph_repair_needed(repo_root, scope, coverage_all) {
        return Ok(());
    }
    let cfg = kiss::GateConfig::load_for_repo(repo_root);
    let covered = workspace_covered_from_stores(repo_root);
    let (key, py, rs) = graph_evidence_parts(repo_root, &cfg, &covered);
    write_graph_items(repo_root, &key, &py, &rs, &cfg.orphan_allowed, &covered).map(|_| ())
}

fn write_graph_items(
    repo_root: &Path,
    key: &str,
    py: &[PathBuf],
    rs: &[PathBuf],
    orphan_allowed: &[String],
    covered: &BTreeMap<String, BTreeSet<u32>>,
) -> Result<Vec<super::graph_store::GraphOrphanItem>, String> {
    super::counters::add_graph();
    crate::test_runner::emit_test_progress("kiss test: graph repair");
    let snapshot = crate::analyze::line_coverage::RuntimeCoverageSnapshot {
        identity: "target-report".into(),
        covered_lines: covered.clone(),
    };
    let findings = crate::analyze::collect_orphan_unit_findings(
        repo_root,
        py,
        rs,
        &snapshot,
        orphan_allowed,
    )
    .map_err(|_| "graph evidence incomplete".to_string())?;
    let items: Vec<super::graph_store::GraphOrphanItem> = findings
        .into_iter()
        .map(|item| {
            let file = item
                .file
                .strip_prefix(repo_root)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| item.file.to_string_lossy().into_owned());
            super::graph_store::GraphOrphanItem {
                file,
                unit_name: item.unit_name,
                start_line: u32::try_from(item.start_line).unwrap_or(u32::MAX),
                end_line: u32::try_from(item.end_line).unwrap_or(u32::MAX),
            }
        })
        .collect();
    super::graph_store::store_items(repo_root, key, items.clone())?;
    Ok(items)
}

pub(crate) fn graph_repair_needed(
    repo_root: &Path,
    scope: &ReportScope,
    coverage_all: bool,
) -> bool {
    graph_generation_id(
        repo_root,
        scope,
        &kiss::GateConfig::load_for_repo(repo_root),
        coverage_all,
        &coverage_maps_from_stores(repo_root, scope).0,
    )
    .is_some_and(|key| super::graph_store::load_items(repo_root, &key).is_none())
}

fn snapshot_token(
    repo_root: &Path,
    request: &super::types::TargetRequest,
    stamp: &TargetSliceStamp,
    evidence: &ReportEvidenceStamp,
    graph_generation: Option<String>,
    gate: &kiss::GateConfig,
    coverage_all: bool,
) -> ReportSnapshot {
    let (python_witness, python_coverage, rust_witness, rust_coverage) =
        language_generation_ids(repo_root);
    ReportSnapshot {
        slice: stamp.clone(),
        evidence: evidence.clone(),
        graph_generation,
        worktree: super::stamp::capture_worktree_token(repo_root),
        gate_policy: gate_policy_id(gate, coverage_all),
        runner: runner_identity(repo_root),
        python_witness,
        python_coverage,
        rust_witness,
        rust_coverage,
        resolved: resolved_dependency_digest(repo_root, request),
        population: population_inventory_id(repo_root, request),
        configuration: configuration_generation(repo_root),
        extra: Vec::new(),
    }
}

fn resolved_dependency_digest(repo_root: &Path, request: &super::types::TargetRequest) -> String {
    let Ok(resolved) = super::resolve::resolve_only(repo_root, request) else {
        return String::new();
    };
    let payload = serde_json::json!({
        "regions": resolved.regions,
        "direct_selectors": resolved.direct_selectors,
        "historical_paths": resolved.historical_paths,
        "git_stamp": resolved.git_stamp,
        "operand_classes": format!("{:?}", resolved.operand_classes),
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("resolved target"))
}

pub(crate) fn runner_identity(repo_root: &Path) -> String {
    let mut parts = Vec::new();
    if let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(repo_root)
    {
        let id = &pinned.plan.base_identity;
        parts.push(serde_json::json!({
            "lang": "python",
            "runner_semantics_version": id.runner_semantics_version,
            "python_version": id.python_version,
            "pytest_version": id.pytest_version,
            "pytest_args": id.pytest_args,
            "interpreter_identity": id.interpreter_identity,
        }));
    }
    if let Ok(witness) = crate::test_runner::lang_rust::try_load_rust_execution_witness(repo_root) {
        parts.push(serde_json::json!({
            "lang": "rust",
            "identity_digest": witness.identity_digest,
        }));
    }
    digest_bytes(&serde_json::to_vec(&parts).expect("runner identity"))
}

fn language_generation_ids(
    repo_root: &Path,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let python_witness =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(
            repo_root,
        )
        .ok()
        .map(|pinned| pinned.generation_id);
    let python_coverage =
        crate::test_runner::python_coverage_index::python_coverage_snapshot_generation_id(
            repo_root,
        );
    let rust_witness = crate::test_runner::lang_rust::try_load_rust_execution_witness(repo_root)
        .ok()
        .map(|witness| witness.generation_id);
    let rust_coverage = crate::test_runner::execution_generation::load_current_generation(
        &crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root),
    )
    .ok()
    .map(|(generation, _)| generation.generation_id);
    (python_witness, python_coverage, rust_witness, rust_coverage)
}

pub(crate) fn evaluation_key_tokens(repo_root: &Path, coverage_all: bool) -> (String, String) {
    (
        runner_identity(repo_root),
        gate_policy_id(&kiss::GateConfig::load_for_repo(repo_root), coverage_all),
    )
}

pub(crate) fn configuration_generation(repo_root: &Path) -> String {
    let bytes = std::fs::read(kiss::kissconfig_path_for_repo(repo_root)).unwrap_or_default();
    digest_bytes(&bytes)
}

fn gate_policy_id(gate: &kiss::GateConfig, coverage_all: bool) -> String {
    let payload = serde_json::json!({
        "test_coverage_threshold": gate.test_coverage_threshold,
        "test_coverage_scope": format!("{:?}", gate.test_coverage_scope),
        "max_unit_test_seconds": gate.max_unit_test_seconds,
        "max_num_tests": gate.max_num_tests,
        "orphan_detection": gate.orphan_detection,
        "orphan_allowed": gate.orphan_allowed,
        "coverage_all": coverage_all,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("gate policy"))
}

fn graph_generation_id(
    repo_root: &Path,
    scope: &ReportScope,
    gate: &kiss::GateConfig,
    coverage_all: bool,
    covered: &BTreeMap<String, BTreeSet<u32>>,
) -> Option<String> {
    if coverage_all || !gate.orphan_detection || !scope_has_orphan_candidates(repo_root, scope)
    {
        return None;
    }
    let (key, py, rs) = graph_evidence_parts(repo_root, gate, covered);
    if py.is_empty() && rs.is_empty() {
        return None;
    }
    Some(key)
}

fn evidence_stamp(
    rows: &[SelectorRow],
    stamp: &TargetSliceStamp,
    exit_code: i32,
    coverage: &ReportCoverage,
    gates: &[ReportGate],
    graph_generation: Option<&str>,
) -> ReportEvidenceStamp {
    evidence_stamp_parts(
        rows,
        stamp,
        exit_code,
        coverage,
        gates,
        graph_generation,
        None,
    )
}

fn evidence_stamp_parts(
    rows: &[SelectorRow],
    stamp: &TargetSliceStamp,
    exit_code: i32,
    coverage: &ReportCoverage,
    gates: &[ReportGate],
    graph_generation: Option<&str>,
    population: Option<&str>,
) -> ReportEvidenceStamp {
    let payload = serde_json::json!({
        "rows": rows,
        "digest": stamp.digest,
        "complete": stamp.complete,
        "exit": exit_code,
        "coverage": coverage,
        "gates": gates,
        "graph_generation": graph_generation,
        "population": population,
    });
    ReportEvidenceStamp {
        digest: digest_bytes(&serde_json::to_vec(&payload).expect("evidence stamp")),
    }
}

fn population_inventory_id(
    repo_root: &Path,
    request: &super::types::TargetRequest,
) -> Option<String> {
    let (need_python, need_rust) = super::manifest::needed_langs(repo_root, request);
    let need = crate::test_runner::workspace_selector_cache::SelectorCountNeed {
        python: need_python && super::manifest::has_python_test_files(repo_root, request),
        rust: need_rust,
    };
    let (py, rs) =
        crate::test_runner::workspace_selector_cache::load_workspace_selectors_for_count(
            repo_root,
            &request.ignore,
            &[],
            need,
        )?;
    let payload = serde_json::json!({
        "python": py,
        "rust": rs,
        "ignore": request.ignore,
        "lang": format!("{:?}", request.lang),
    });
    Some(digest_bytes(
        &serde_json::to_vec(&payload).expect("population inventory"),
    ))
}

fn worst_status(left: EffectiveStatus, right: EffectiveStatus) -> EffectiveStatus {
    match (left, right) {
        (EffectiveStatus::Timeout, _) | (_, EffectiveStatus::Timeout) => EffectiveStatus::Timeout,
        (EffectiveStatus::Fail, _) | (_, EffectiveStatus::Fail) => EffectiveStatus::Fail,
        (EffectiveStatus::Pass, EffectiveStatus::Pass) => EffectiveStatus::Pass,
    }
}

#[cfg(test)]
mod coverage_focus_tests {
    use super::*;
    use crate::test_runner::target_request::scope::ReportScope;

    fn file_lines(path: &str, lines: &[u32]) -> SourceRegion {
        SourceRegion::FileLines {
            path: path.into(),
            lines: lines.iter().copied().collect(),
        }
    }

    #[test]
    fn file_lines_ignore_off_focus_uncovered() {
        let scope = ReportScope::from_membership(vec![file_lines("a.py", &[1, 2])], vec![], true);
        let mut covered: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        covered.insert("a.py".into(), [1, 2].into_iter().collect());
        let mut coverable: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        coverable.insert("a.py".into(), [1, 2, 3].into_iter().collect());
        let focused_coverable = {
            let mut dest = BTreeMap::new();
            extend_focused(&scope, "a.py", coverable["a.py"].iter().copied(), &mut dest);
            dest
        };
        let focused_covered = {
            let mut dest = BTreeMap::new();
            extend_focused(&scope, "a.py", covered["a.py"].iter().copied(), &mut dest);
            dest
        };
        let cov = coverage_from_maps(&scope, focused_covered, focused_coverable);
        assert_eq!(cov.coverable, 2);
        assert_eq!(cov.covered, 2);
        assert!(cov.uncovered.is_empty(), "{:?}", cov.uncovered);
    }

    #[test]
    fn file_lines_uncovered_when_focus_misses() {
        let scope = ReportScope::from_membership(vec![file_lines("a.py", &[1, 2])], vec![], true);
        let mut dest = BTreeMap::new();
        extend_focused(&scope, "a.py", [1], &mut dest);
        let cov = coverage_from_maps(&scope, dest, BTreeMap::new());
        assert_eq!(cov.uncovered, vec!["a.py".to_string()]);
    }

    #[test]
    fn file_all_uncovered_when_coverable_misses() {
        let scope = ReportScope::from_membership(
            vec![SourceRegion::FileAll {
                path: "a.py".into(),
            }],
            vec![],
            true,
        );
        let mut covered = BTreeMap::new();
        covered.insert("a.py".into(), [1, 2].into_iter().collect());
        let mut coverable = BTreeMap::new();
        coverable.insert("a.py".into(), [1, 2, 3].into_iter().collect());
        let cov = coverage_from_maps(&scope, covered, coverable);
        assert_eq!(cov.uncovered, vec!["a.py".to_string()]);
    }

    #[test]
    fn file_all_coverable_from_current_source() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def foo():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(
            vec![SourceRegion::FileAll {
                path: "app.py".into(),
            }],
            vec![],
            true,
        );
        let cov = coverage_from_stores(tmp.path(), &scope);
        assert!(
            cov.coverable > 0,
            "current-source parse must yield coverable lines; {cov:?}"
        );
    }

    #[test]
    fn workspace_all_coverable_from_current_source() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def foo():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let cov = coverage_from_stores(tmp.path(), &scope);
        assert!(
            cov.coverable > 0,
            "workspace current-source parse must yield coverable lines; {cov:?}"
        );
        assert_eq!(cov.uncovered, vec!["app.py".to_string()]);
    }

    #[test]
    fn test_only_file_all_has_empty_coverage_obligation() {
        let scope = ReportScope::from_membership(
            vec![SourceRegion::FileAll {
                path: "test_lib.py::test_fast".into(),
            }],
            vec!["test_lib.py::test_fast".into()],
            true,
        );
        let cov = coverage_from_maps(&scope, BTreeMap::new(), BTreeMap::new());
        assert!(
            cov.uncovered.is_empty(),
            "test-only target has no production coverage obligation: {:?}",
            cov.uncovered
        );
    }

    #[test]
    fn workspace_all_skips_test_module_uncovered() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def foo():\n    return 1\n").unwrap();
        std::fs::write(
            tmp.path().join("test_lib.py"),
            "def test_fast():\n    assert True\n",
        )
        .unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let cov = coverage_from_stores(tmp.path(), &scope);
        assert_eq!(cov.uncovered, vec!["app.py".to_string()]);
        assert!(
            !cov.uncovered.iter().any(|path| path.contains("test_")),
            "{:?}",
            cov.uncovered
        );
    }

    fn sample_uncovered() -> ReportCoverage {
        ReportCoverage {
            covered: 95,
            coverable: 100,
            uncovered: vec!["bad.py".into()],
            files: Vec::new(),
        }
    }

    fn sample_file(covered: u64, coverable: u64) -> ReportCoverage {
        ReportCoverage {
            covered,
            coverable,
            uncovered: vec!["bad.py".into()],
            files: vec![ReportFileCoverage {
                path: "bad.py".into(),
                covered,
                coverable,
            }],
        }
    }

    #[test]
    fn codebase_scope_empty_gates_when_aggregate_clears() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 90,
            test_coverage_scope: kiss::TestCoverageScope::Codebase,
            ..Default::default()
        };
        assert!(
            gates_from_coverage(&sample_uncovered(), &gate).is_empty(),
            "codebase aggregate 95% must not emit per-file gates"
        );
    }

    #[test]
    fn by_file_scope_keeps_uncovered_file() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 90,
            test_coverage_scope: kiss::TestCoverageScope::ByFile,
            ..Default::default()
        };
        let gates = gates_from_coverage(&sample_uncovered(), &gate);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].detail, "bad.py");
    }

    #[test]
    fn by_file_scope_empty_gates_when_file_meets_threshold() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 90,
            test_coverage_scope: kiss::TestCoverageScope::ByFile,
            ..Default::default()
        };
        assert!(
            gates_from_coverage(&sample_file(95, 100), &gate).is_empty(),
            "ByFile 95% must not emit a gate at a 90% threshold"
        );
    }

    #[test]
    fn by_file_scope_keeps_file_below_threshold() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 90,
            test_coverage_scope: kiss::TestCoverageScope::ByFile,
            ..Default::default()
        };
        let gates = gates_from_coverage(&sample_file(50, 100), &gate);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].detail, "bad.py");
    }

    #[test]
    fn threshold_zero_emits_no_coverage_gates() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 0,
            test_coverage_scope: kiss::TestCoverageScope::ByFile,
            ..Default::default()
        };
        assert!(gates_from_coverage(&sample_uncovered(), &gate).is_empty());
    }

    #[test]
    fn coverage_all_emits_uncovered_despite_threshold() {
        let gate = kiss::GateConfig {
            test_coverage_threshold: 90,
            test_coverage_scope: kiss::TestCoverageScope::Codebase,
            ..Default::default()
        };
        let cov = sample_uncovered();
        assert!(
            gates_from_coverage(&cov, &gate).is_empty(),
            "threshold 90% must accept 95%"
        );
        let gates = gates_from_coverage_all(&cov);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "test_coverage");
        assert_eq!(gates[0].detail, "bad.py");
    }
}

#[cfg(test)]
mod exit_gate_tests {
    use super::*;
    use crate::test_runner::target_request::scope::ReportScope;
    use crate::test_runner::target_request::slice::TARGET_SLICE_SCHEMA;

    fn stamp() -> TargetSliceStamp {
        TargetSliceStamp {
            digest: "d".into(),
            complete: true,
            index_schema: TARGET_SLICE_SCHEMA.into(),
        }
    }

    fn pass_row() -> SelectorRow {
        SelectorRow {
            language: "python".into(),
            selector: "tests/a.py::test_a".into(),
            raw: "passed".into(),
            effective: EffectiveStatus::Pass,
            duration_ns: None,
            provenance: "witness".into(),
        }
    }

    fn timeout_row() -> SelectorRow {
        SelectorRow {
            language: "python".into(),
            selector: "tests/b.py::test_b".into(),
            raw: "timed_out".into(),
            effective: EffectiveStatus::Timeout,
            duration_ns: None,
            provenance: "witness".into(),
        }
    }

    fn coverage_gate() -> ReportGate {
        ReportGate {
            kind: "test_coverage".into(),
            detail: "a.py".into(),
        }
    }

    #[test]
    fn apply_gate_exit_keeps_pass_without_gates() {
        assert_eq!(TargetReport::apply_gate_exit(0, &[]), 0);
    }

    #[test]
    fn apply_gate_exit_fails_pass_when_coverage_gate_present() {
        assert_eq!(TargetReport::apply_gate_exit(0, &[coverage_gate()]), 1);
    }

    #[test]
    fn coverage_gate_fails_pass_exit() {
        let report = TargetReport::assembled_with(
            ReportScope::from_membership(Vec::new(), vec!["tests/a.py::test_a".into()], true),
            vec![pass_row()],
            stamp(),
            0,
            ReportCoverage::default(),
            vec![coverage_gate()],
            false,
        );
        assert_eq!(report.exit_code, 1);
    }

    #[test]
    fn coverage_gate_does_not_override_timeout() {
        let report = TargetReport::assembled_with(
            ReportScope::from_membership(Vec::new(), vec!["tests/b.py::test_b".into()], true),
            vec![timeout_row()],
            stamp(),
            124,
            ReportCoverage::default(),
            vec![coverage_gate()],
            false,
        );
        assert_eq!(report.exit_code, 124);
    }

    fn timed_row(selector: &str, duration_ns: u64) -> SelectorRow {
        SelectorRow {
            language: "python".into(),
            selector: selector.into(),
            raw: "passed".into(),
            effective: EffectiveStatus::Pass,
            duration_ns: Some(duration_ns),
            provenance: "witness".into(),
        }
    }

    #[test]
    fn in_scope_slow_row_emits_time_gate() {
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 2.0)],
            ..Default::default()
        };
        let gates = gates_from_timing(&[timed_row("tests/a.py::test_a", 2_000_000_000)], &gate);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "max_unit_test_seconds");
        assert_eq!(gates[0].detail, "tests/a.py::test_a");
    }

    #[test]
    fn faster_in_scope_row_emits_no_time_gate() {
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 2.0)],
            ..Default::default()
        };
        assert!(
            gates_from_timing(&[timed_row("tests/a.py::test_a", 1_000_000_000)], &gate).is_empty()
        );
    }

    #[test]
    fn missing_duration_emits_no_time_gate() {
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 2.0)],
            ..Default::default()
        };
        assert!(gates_from_timing(&[pass_row()], &gate).is_empty());
    }

    #[test]
    fn time_gate_fails_pass_exit() {
        let report = TargetReport::assembled_with(
            ReportScope::from_membership(Vec::new(), vec!["tests/a.py::test_a".into()], true),
            vec![pass_row()],
            stamp(),
            0,
            ReportCoverage::default(),
            vec![ReportGate {
                kind: "max_unit_test_seconds".into(),
                detail: "tests/a.py::test_a".into(),
            }],
            false,
        );
        assert_eq!(report.exit_code, 1);
    }

    fn workspace_request() -> super::super::types::TargetRequest {
        super::super::canon::canonicalize_target_request(
            super::super::types::TargetRequest {
                focus: super::super::types::TargetFocus::Workspace,
                lang: None,
                ignore: Vec::new(),
            },
            None,
        )
    }

    fn seed_count_repo(tmp: &tempfile::TempDir) {
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(
            tmp.path().join("tests/test_a.py"),
            "def test_a():\n    assert True\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    }

    #[test]
    fn missing_population_cache_fails_closed() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_count_repo(&tmp);
        let gates = gates_from_population(
            tmp.path(),
            &kiss::GateConfig::default(),
            &workspace_request(),
        );
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "max_num_tests");
        assert_eq!(gates[0].detail, "population evidence incomplete");
    }

    #[test]
    fn workspace_count_over_limit_emits_max_num_tests_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_count_repo(&tmp);
        assert!(
            crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
                tmp.path(),
                &[],
                &["tests/a.py::test_a".into(), "tests/b.py::test_b".into()],
                &[],
            )
        );
        assert!(
            crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
                tmp.path(),
                &[],
                &["src/lib.rs::t".into()],
            )
        );
        let gate = kiss::GateConfig {
            max_num_tests: 2,
            ..Default::default()
        };
        let gates = gates_from_population(tmp.path(), &gate, &workspace_request());
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "max_num_tests");
        assert_eq!(gates[0].detail, "3 test(s) exceeds max_num_tests=2");
    }

    #[test]
    fn rust_lang_filter_does_not_count_python_population() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_count_repo(&tmp);
        assert!(
            crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
                tmp.path(),
                &[],
                &["tests/a.py::test_a".into(), "tests/b.py::test_b".into()],
                &[],
            )
        );
        assert!(
            crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
                tmp.path(),
                &[],
                &["src/lib.rs::t".into()],
            )
        );
        let mut request = workspace_request();
        request.lang = Some(super::super::types::LangFilter::Rust);
        let gate = kiss::GateConfig {
            max_num_tests: 2,
            ..Default::default()
        };
        assert!(gates_from_population(tmp.path(), &gate, &request).is_empty());
    }

    #[test]
    fn disabled_orphan_emits_no_gates() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: false,
            ..Default::default()
        };
        assert!(gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new()).is_empty());
    }

    fn seed_orphan_graph(
        repo: &std::path::Path,
        _scope: &ReportScope,
        gate: &kiss::GateConfig,
        covered: &BTreeMap<String, BTreeSet<u32>>,
    ) {
        let (key, py, rs) = graph_evidence_parts(repo, gate, covered);
        write_graph_items(repo, &key, &py, &rs, &gate.orphan_allowed, covered).unwrap();
    }

    #[test]
    fn unused_python_helper_emits_orphan_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        let gates = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(gates.iter().any(|item| item.kind == "orphan"), "{gates:?}");
    }

    #[test]
    fn orphan_gate_miss_fails_closed() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        let gates = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "orphan");
        assert_eq!(gates[0].detail, "graph evidence incomplete");
    }

    #[test]
    fn covered_python_helper_emits_no_orphan_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("utils.py"),
            "x = 1\ndef helper():\n    return 1\n",
        )
        .unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        let mut covered = BTreeMap::new();
        covered.insert("utils.py".into(), BTreeSet::from([1, 2, 3]));
        seed_orphan_graph(tmp.path(), &scope, &gate, &covered);
        assert!(gates_from_orphan(tmp.path(), &gate, &scope, &covered).is_empty());
    }

    #[test]
    fn coverage_all_skips_orphan_and_population() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        seed_count_repo(&tmp);
        assert!(
            crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
                tmp.path(),
                &[],
                &["tests/a.py::test_a".into(), "tests/b.py::test_b".into()],
                &[],
            )
        );
        assert!(
            crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
                tmp.path(),
                &[],
                &["src/lib.rs::t".into()],
            )
        );
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            max_num_tests: 1,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        assert!(!gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new()).is_empty());
        assert!(!gates_from_population(tmp.path(), &gate, &workspace_request()).is_empty());
        let report = TargetReport::assembled_in(
            tmp.path(),
            &workspace_request(),
            scope,
            Vec::new(),
            stamp(),
            0,
            true,
        );
        assert!(
            !report
                .gates
                .iter()
                .any(|item| item.kind == "orphan" || item.kind == "max_num_tests"),
            "{:?}",
            report.gates
        );
        assert!(report.coverage_all);
    }

    #[test]
    fn coverage_all_keeps_time_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test.max_unit_test_seconds]\n\"*\" = 2.0\n",
        )
        .unwrap();
        let scope =
            ReportScope::from_membership(Vec::new(), vec!["tests/a.py::test_a".into()], true);
        let report = TargetReport::assembled_in(
            tmp.path(),
            &workspace_request(),
            scope,
            vec![timed_row("tests/a.py::test_a", 2_000_000_000)],
            stamp(),
            0,
            true,
        );
        assert!(
            report
                .gates
                .iter()
                .any(|item| item.kind == "max_unit_test_seconds"),
            "{:?}",
            report.gates
        );
    }

    #[test]
    fn graph_cache_hit_returns_stored_items() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        let (key, _, _) = graph_evidence_parts(tmp.path(), &gate, &BTreeMap::new());
        super::super::graph_store::store_items(
            tmp.path(),
            &key,
            vec![super::super::graph_store::GraphOrphanItem {
                file: "utils.py".into(),
                unit_name: "planted".into(),
                start_line: 1,
                end_line: 2,
            }],
        )
        .unwrap();
        let gates = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(
            gates.iter().any(|item| item.detail.contains("planted")),
            "{gates:?}"
        );
        assert!(
            !gates.iter().any(|item| item.detail.contains("helper")),
            "{gates:?}"
        );
    }

    #[test]
    fn graph_cache_misses_after_source_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        let (key, _, _) = graph_evidence_parts(tmp.path(), &gate, &BTreeMap::new());
        super::super::graph_store::store_items(
            tmp.path(),
            &key,
            vec![super::super::graph_store::GraphOrphanItem {
                file: "utils.py".into(),
                unit_name: "planted".into(),
                start_line: 1,
                end_line: 2,
            }],
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("utils.py"),
            "x = 1\ndef helper():\n    return 1\n",
        )
        .unwrap();
        let miss = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert_eq!(miss[0].detail, "graph evidence incomplete", "{miss:?}");
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        let gates = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(
            !gates.iter().any(|item| item.detail.contains("planted")),
            "{gates:?}"
        );
        assert!(gates.iter().any(|item| item.kind == "orphan"), "{gates:?}");
    }

    fn focused_utils_scope() -> ReportScope {
        ReportScope::from_membership(
            vec![SourceRegion::FileAll {
                path: "utils.py".into(),
            }],
            vec![],
            true,
        )
    }

    #[test]
    fn external_reference_clears_focused_orphan() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        let scope = focused_utils_scope();
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        let alone = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(
            alone
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.starts_with("utils.py:")),
            "{alone:?}"
        );
        std::fs::write(
            tmp.path().join("app.py"),
            "from utils import helper\nif __name__ == \"__main__\":\n    helper()\n",
        )
        .unwrap();
        let miss = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert_eq!(miss[0].detail, "graph evidence incomplete", "{miss:?}");
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        let referenced = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(
            !referenced
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.starts_with("utils.py:")),
            "{referenced:?}"
        );
    }

    #[test]
    fn focused_orphan_omits_external_unit() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        std::fs::write(tmp.path().join("other.py"), "def unused():\n    return 2\n").unwrap();
        let scope = focused_utils_scope();
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate, &BTreeMap::new());
        let gates = gates_from_orphan(tmp.path(), &gate, &scope, &BTreeMap::new());
        assert!(
            gates
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.starts_with("utils.py:")),
            "{gates:?}"
        );
        assert!(
            !gates.iter().any(|item| item.detail.contains("unused")),
            "{gates:?}"
        );
    }

    #[test]
    fn file_lines_orphan_omits_sibling_unit() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("utils.py"),
            "def used():\n    return 1\n\ndef unused():\n    return 2\n",
        )
        .unwrap();
        let mut covered = BTreeMap::new();
        covered.insert("utils.py".into(), BTreeSet::from([1, 2]));
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        let used = ReportScope::from_membership(
            vec![SourceRegion::FileLines {
                path: "utils.py".into(),
                lines: BTreeSet::from([1, 2]),
            }],
            vec![],
            true,
        );
        let unused = ReportScope::from_membership(
            vec![SourceRegion::FileLines {
                path: "utils.py".into(),
                lines: BTreeSet::from([4, 5]),
            }],
            vec![],
            true,
        );
        seed_orphan_graph(tmp.path(), &used, &gate, &covered);
        let used_gates = gates_from_orphan(tmp.path(), &gate, &used, &covered);
        assert!(
            !used_gates
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.contains("unused")),
            "{used_gates:?}"
        );
        let unused_gates = gates_from_orphan(tmp.path(), &gate, &unused, &covered);
        assert!(
            unused_gates
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.contains("unused")),
            "{unused_gates:?}"
        );
    }
}
