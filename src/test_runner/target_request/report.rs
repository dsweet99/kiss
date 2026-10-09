#![cfg_attr(not(test), allow(dead_code))]
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::report_gates::{
    gates_from_orphan, gates_from_population, gates_from_timing, graph_evidence_parts,
    scope_has_orphan_candidates, workspace_source_files,
};
#[cfg(test)]
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
    pub language: kiss::Language,
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
    pub graph_mutable: Option<String>,
    #[serde(default)]
    pub worktree: String,
    #[serde(default)]
    pub gate_policy: String,
    #[serde(default)]
    pub runner: String,
    #[serde(default)]
    pub generations: crate::test_runner::language_keyed::LanguageKeyed<
        crate::test_runner::lang_iface::GenerationIds,
    >,
    #[serde(default)]
    pub resolved: String,
    #[serde(default)]
    pub population: Option<String>,
    #[serde(default)]
    pub configuration: String,
    #[serde(default)]
    pub extras: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
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
    pub gates: Vec<ReportGate>,
    #[serde(default)]
    pub graph_generation: Option<String>,
    #[serde(default)]
    pub snapshot: ReportSnapshot,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
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
    ) -> Self {
        let cfg = kiss::GateConfig::load_for_repo(repo_root);
        let mut gates = gates_from_timing(&rows, &cfg);
        gates.extend(gates_from_population(repo_root, &cfg, request));
        gates.extend(gates_from_orphan(repo_root, &cfg, &scope));
        let graph_generation = graph_generation_id(repo_root, &scope, &cfg);
        let configuration = configuration_generation(repo_root);
        let graph_mutable = graph_generation.as_ref().map(|_| {
            super::graph_store::evidence_mutable_digest(&cfg.orphan_allowed, &configuration)
        });
        let population = population_inventory_id(repo_root, request);
        let mut built = Self::assembled_with(scope, rows, stamp, exit_code, gates);
        built.evidence = evidence_stamp_parts(
            &built.rows,
            &built.stamp,
            built.exit_code,
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
        );
        built.snapshot.graph_mutable = graph_mutable;
        let selectors: Vec<String> = built.rows.iter().map(|row| row.selector.clone()).collect();
        built.labels = crate::test_runner::lang_registry::languages()
            .into_iter()
            .flat_map(|language| {
                crate::test_runner::lang_registry::rules_for(language)
                    .report_labels(repo_root, &selectors)
            })
            .collect();
        built
    }

    fn assembled_with(
        scope: ReportScope,
        rows: Vec<SelectorRow>,
        stamp: TargetSliceStamp,
        exit_code: i32,
        gates: Vec<ReportGate>,
    ) -> Self {
        let exit_code = Self::apply_gate_exit(exit_code, &gates);
        let evidence = evidence_stamp(&rows, &stamp, exit_code, &gates, None);
        Self {
            scope,
            rows,
            stamp,
            exit_code,
            evidence,
            gates,
            graph_generation: None,
            snapshot: ReportSnapshot::default(),
            labels: BTreeMap::new(),
        }
    }

    pub(crate) fn exit_for(worst: Option<EffectiveStatus>) -> i32 {
        match worst {
            Some(EffectiveStatus::Timeout) => 1,
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
                "max_unit_test_seconds" | "max_num_tests" | "orphan"
            )
        }) {
            Self::combine_exit(exit_code, 1)
        } else {
            exit_code
        }
    }

    pub(crate) fn combine_exit(row_exit: i32, caller_exit: i32) -> i32 {
        let row_exit = if row_exit == 124 { 1 } else { row_exit };
        let caller_exit = if caller_exit == 124 { 1 } else { caller_exit };
        if row_exit != 0 { row_exit } else { caller_exit }
    }
}

pub(crate) fn repair_graph_evidence(repo_root: &Path, scope: &ReportScope) -> Result<(), String> {
    if !graph_repair_needed(repo_root, scope) {
        return Ok(());
    }
    let cfg = kiss::GateConfig::load_for_repo(repo_root);
    let (key, py, rs) = graph_evidence_parts(repo_root, &cfg);
    write_graph_items(repo_root, &key, &py, &rs, &cfg.orphan_allowed).map(|_| ())
}

fn write_graph_items(
    repo_root: &Path,
    key: &str,
    py: &[PathBuf],
    rs: &[PathBuf],
    orphan_allowed: &[String],
) -> Result<Vec<super::graph_store::GraphOrphanItem>, String> {
    super::counters::add_graph();
    crate::test_runner::emit_test_progress("kiss test: graph repair");
    let findings = crate::analyze::collect_orphan_unit_findings(repo_root, py, rs, orphan_allowed)
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

pub(crate) fn graph_repair_needed(repo_root: &Path, scope: &ReportScope) -> bool {
    graph_generation_id(
        repo_root,
        scope,
        &kiss::GateConfig::load_for_repo(repo_root),
    )
    .is_some_and(|key| super::graph_store::load_items(repo_root, &key).is_none())
}

fn graph_generation_active(repo_root: &Path, scope: &ReportScope, gate: &kiss::GateConfig) -> bool {
    if !gate.orphan_detection || !scope_has_orphan_candidates(repo_root, scope) {
        return false;
    }
    let (py, rs) = workspace_source_files(repo_root);
    !py.is_empty() || !rs.is_empty()
}

fn snapshot_token(
    repo_root: &Path,
    request: &super::types::TargetRequest,
    stamp: &TargetSliceStamp,
    evidence: &ReportEvidenceStamp,
    graph_generation: Option<String>,
    gate: &kiss::GateConfig,
) -> ReportSnapshot {
    ReportSnapshot {
        slice: stamp.clone(),
        evidence: evidence.clone(),
        graph_generation,
        graph_mutable: None,
        worktree: super::stamp::capture_worktree_token(repo_root, request.language()),
        gate_policy: gate_policy_id(gate),
        runner: runner_identity(repo_root),
        generations: language_generation_ids(repo_root),
        resolved: resolved_dependency_digest(repo_root, request),
        population: population_inventory_id(repo_root, request),
        configuration: configuration_generation(repo_root),
        extras: crate::test_runner::language_keyed::LanguageKeyed::default(),
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
    digest_bytes(&serde_json::to_vec(&runner_identity_parts(repo_root)).expect("runner identity"))
}

fn runner_identity_parts(repo_root: &Path) -> Vec<serde_json::Value> {
    crate::test_runner::lang_registry::all_rules()
        .filter_map(|rules| rules.runner_identity_part(repo_root))
        .collect()
}

fn language_generation_ids(
    repo_root: &Path,
) -> crate::test_runner::language_keyed::LanguageKeyed<crate::test_runner::lang_iface::GenerationIds>
{
    crate::test_runner::language_keyed::LanguageKeyed::from_fn(|language| {
        crate::test_runner::lang_registry::rules_for(language).generation_ids(repo_root)
    })
}

pub(super) fn configuration_generation(repo_root: &Path) -> String {
    let bytes = std::fs::read(kiss::kissconfig_path_for_repo(repo_root)).unwrap_or_default();
    digest_bytes(&bytes)
}

fn gate_policy_id(gate: &kiss::GateConfig) -> String {
    let payload = serde_json::json!({
        "max_unit_test_seconds": gate.max_unit_test_seconds,
        "max_num_tests": gate.max_num_tests,
        "orphan_detection": gate.orphan_detection,
        "orphan_allowed": gate.orphan_allowed,
    });
    digest_bytes(&serde_json::to_vec(&payload).expect("gate policy"))
}

fn graph_generation_id(
    repo_root: &Path,
    scope: &ReportScope,
    gate: &kiss::GateConfig,
) -> Option<String> {
    if !graph_generation_active(repo_root, scope, gate) {
        return None;
    }
    let (key, _, _) = graph_evidence_parts(repo_root, gate);
    Some(key)
}

fn evidence_stamp(
    rows: &[SelectorRow],
    stamp: &TargetSliceStamp,
    exit_code: i32,
    gates: &[ReportGate],
    graph_generation: Option<&str>,
) -> ReportEvidenceStamp {
    evidence_stamp_parts(rows, stamp, exit_code, gates, graph_generation, None)
}

fn evidence_stamp_parts(
    rows: &[SelectorRow],
    stamp: &TargetSliceStamp,
    exit_code: i32,
    gates: &[ReportGate],
    graph_generation: Option<&str>,
    population: Option<&str>,
) -> ReportEvidenceStamp {
    let payload = serde_json::json!({
        "rows": rows,
        "digest": stamp.digest,
        "complete": stamp.complete,
        "exit": exit_code,
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
    let need = super::manifest::population_count_need(repo_root, request);
    let (first, second) =
        crate::test_runner::workspace_selector_cache::load_workspace_selectors_for_count(
            repo_root,
            &request.ignore,
            &[],
            need,
        )?;
    let payload = serde_json::json!({
        "selectors": [first, second],
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
mod exit_gate_tests {
    use super::super::report_gates::{
        gates_from_orphan, gates_from_population, gates_from_timing, graph_evidence_parts,
    };
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
            language: kiss::Language::Python,
            selector: "tests/a.py::test_a".into(),
            raw: "passed".into(),
            effective: EffectiveStatus::Pass,
            duration_ns: None,
            provenance: "witness".into(),
        }
    }

    fn timeout_row() -> SelectorRow {
        SelectorRow {
            language: kiss::Language::Python,
            selector: "tests/b.py::test_b".into(),
            raw: "timed_out".into(),
            effective: EffectiveStatus::Timeout,
            duration_ns: None,
            provenance: "witness".into(),
        }
    }

    fn orphan_gate() -> ReportGate {
        ReportGate {
            kind: "orphan".into(),
            detail: "a.py:helper".into(),
        }
    }

    #[test]
    fn apply_gate_exit_ignores_unknown_gate_kind() {
        let gate = ReportGate {
            kind: "unknown".into(),
            detail: "a.py".into(),
        };
        assert_eq!(TargetReport::apply_gate_exit(0, &[gate]), 0);
    }

    #[test]
    fn apply_gate_exit_keeps_pass_without_gates() {
        assert_eq!(TargetReport::apply_gate_exit(0, &[]), 0);
    }

    #[test]
    fn apply_gate_exit_fails_pass_when_orphan_gate_present() {
        assert_eq!(TargetReport::apply_gate_exit(0, &[orphan_gate()]), 1);
    }

    #[test]
    fn orphan_gate_fails_pass_exit() {
        let report = TargetReport::assembled_with(
            ReportScope::from_membership(Vec::new(), vec!["tests/a.py::test_a".into()], true),
            vec![pass_row()],
            stamp(),
            0,
            vec![orphan_gate()],
        );
        assert_eq!(report.exit_code, 1);
    }

    #[test]
    fn orphan_gate_does_not_override_timeout() {
        let report = TargetReport::assembled_with(
            ReportScope::from_membership(Vec::new(), vec!["tests/b.py::test_b".into()], true),
            vec![timeout_row()],
            stamp(),
            124,
            vec![orphan_gate()],
        );
        assert_eq!(report.exit_code, 1);
    }

    fn timed_row(selector: &str, duration_ns: u64) -> SelectorRow {
        SelectorRow {
            language: kiss::Language::Python,
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
            vec![ReportGate {
                kind: "max_unit_test_seconds".into(),
                detail: "tests/a.py::test_a".into(),
            }],
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
    fn missing_population_cache_is_filled_by_enumeration() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_count_repo(&tmp);
        let gates = gates_from_population(
            tmp.path(),
            &kiss::GateConfig::default(),
            &workspace_request(),
        );
        assert!(
            gates
                .iter()
                .all(|gate| gate.detail != "population evidence incomplete"),
            "{gates:?}"
        );
        assert!(
            crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors(
                tmp.path(),
                &[],
                &[],
            )
            .is_some(),
            "enumeration stores the population for the next count"
        );
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
        assert!(gates_from_orphan(tmp.path(), &gate, &scope).is_empty());
    }

    fn seed_orphan_graph(repo: &std::path::Path, _scope: &ReportScope, gate: &kiss::GateConfig) {
        let (key, py, rs) = graph_evidence_parts(repo, gate);
        write_graph_items(repo, &key, &py, &rs, &gate.orphan_allowed).unwrap();
    }

    fn write_used_test(repo: &Path, name: &str) {
        let tests = repo.join("tests");
        std::fs::create_dir_all(&tests).unwrap();
        std::fs::write(
            tests.join("test_utils.py"),
            format!("from utils import {name}\n\ndef test_{name}():\n    assert {name}()\n"),
        )
        .unwrap();
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
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let gates = gates_from_orphan(tmp.path(), &gate, &scope);
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
        let gates = gates_from_orphan(tmp.path(), &gate, &scope);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].kind, "orphan");
        assert_eq!(gates[0].detail, "graph evidence incomplete");
    }

    #[test]
    fn test_referenced_python_helper_emits_no_orphan_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        write_used_test(tmp.path(), "helper");
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate);
        assert!(gates_from_orphan(tmp.path(), &gate, &scope).is_empty());
    }

    #[test]
    fn assembled_report_keeps_orphan_and_population_gates() {
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
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\norphan_detection = true\nmax_num_tests = 1\n",
        )
        .unwrap();
        let scope = ReportScope::from_membership(vec![SourceRegion::WorkspaceAll], vec![], true);
        let gate = kiss::GateConfig::load_for_repo(tmp.path());
        assert!(gate.orphan_detection && gate.max_num_tests == 1);
        seed_orphan_graph(tmp.path(), &scope, &gate);
        assert!(!gates_from_orphan(tmp.path(), &gate, &scope).is_empty());
        assert!(!gates_from_population(tmp.path(), &gate, &workspace_request()).is_empty());
        let report = TargetReport::assembled_in(
            tmp.path(),
            &workspace_request(),
            scope,
            Vec::new(),
            stamp(),
            0,
        );
        assert!(
            report.gates.iter().any(|item| item.kind == "orphan"),
            "{:?}",
            report.gates
        );
        assert!(
            report.gates.iter().any(|item| item.kind == "max_num_tests"),
            "{:?}",
            report.gates
        );
        assert_eq!(report.exit_code, 1);
    }

    #[test]
    fn assembled_report_emits_time_gate() {
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
        let (key, _, _) = graph_evidence_parts(tmp.path(), &gate);
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
        let gates = gates_from_orphan(tmp.path(), &gate, &scope);
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
        let (key, _, _) = graph_evidence_parts(tmp.path(), &gate);
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
        let miss = gates_from_orphan(tmp.path(), &gate, &scope);
        assert_eq!(miss[0].detail, "graph evidence incomplete", "{miss:?}");
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let gates = gates_from_orphan(tmp.path(), &gate, &scope);
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
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let alone = gates_from_orphan(tmp.path(), &gate, &scope);
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
        let miss = gates_from_orphan(tmp.path(), &gate, &scope);
        assert_eq!(miss[0].detail, "graph evidence incomplete", "{miss:?}");
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let referenced = gates_from_orphan(tmp.path(), &gate, &scope);
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
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let gates = gates_from_orphan(tmp.path(), &gate, &scope);
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

    /// Focused FileAll reports still key graph evidence on the full workspace source set:
    /// an out-of-scope production edit must miss so reachability can be recomputed
    /// (see `external_reference_clears_focused_orphan`). Closing kt_bug.md "scoped key" claim.
    #[test]
    fn focused_graph_evidence_misses_after_out_of_scope_source_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        std::fs::write(tmp.path().join("other.py"), "def unused():\n    return 2\n").unwrap();
        let scope = focused_utils_scope();
        let gate = kiss::GateConfig {
            orphan_detection: true,
            ..Default::default()
        };
        seed_orphan_graph(tmp.path(), &scope, &gate);
        let before = gates_from_orphan(tmp.path(), &gate, &scope);
        assert!(
            before
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.starts_with("utils.py:")),
            "{before:?}"
        );
        std::fs::write(tmp.path().join("other.py"), "def unused():\n    return 3\n").unwrap();
        let after = gates_from_orphan(tmp.path(), &gate, &scope);
        assert_eq!(
            after[0].detail, "graph evidence incomplete",
            "out-of-scope production edit must invalidate focused graph evidence: {after:?}"
        );
    }

    /// Behavioral lock for kt_bug.md: after repair stores under the workspace key,
    /// a focused FileAll need-check must not miss when out-of-scope sources are present.
    #[test]
    fn focused_graph_repair_stays_warm_with_out_of_scope_sources() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        std::fs::write(tmp.path().join("other.py"), "def unused():\n    return 2\n").unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\norphan_detection = true\n",
        )
        .unwrap();
        let scope = focused_utils_scope();
        assert!(
            graph_repair_needed(tmp.path(), &scope),
            "first focused cycle must need repair when evidence is absent"
        );
        repair_graph_evidence(tmp.path(), &scope).unwrap();
        assert!(
            !graph_repair_needed(tmp.path(), &scope),
            "after workspace-keyed repair, focused need-check must hit the same ITE key"
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
        write_used_test(tmp.path(), "used");
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
        seed_orphan_graph(tmp.path(), &used, &gate);
        let used_gates = gates_from_orphan(tmp.path(), &gate, &used);
        assert!(
            !used_gates
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.contains("unused")),
            "{used_gates:?}"
        );
        let unused_gates = gates_from_orphan(tmp.path(), &gate, &unused);
        assert!(
            unused_gates
                .iter()
                .any(|item| item.kind == "orphan" && item.detail.contains("unused")),
            "{unused_gates:?}"
        );
    }
}
