use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kiss::{
    OrphanCoverage, OrphanUnitInput, build_python_context_graph, build_rust_context_graph,
    collect_orphan_entry_paths, orphan_unit_violations,
};

use crate::analyze::line_coverage::{
    CoverageSourceFacts, RuntimeCoverageSnapshot, repo_relative_key,
};
use crate::analyze_parse::parse_classified;

pub(crate) fn evaluate_orphan_unit_gate(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    snapshot: &RuntimeCoverageSnapshot,
    gate: &kiss::GateConfig,
    bypass: bool,
) -> bool {
    if bypass || !gate.orphan_unit_enabled {
        return false;
    }
    let Ok((py_parsed, rs_parsed, roles)) = parse_classified(py_files, rs_files) else {
        eprintln!("error: kiss test: failed to parse sources for orphan units");
        return true;
    };
    let facts = CoverageSourceFacts::from_index(roles.clone(), &rs_parsed, py_files, rs_files);
    let coverage = snapshot_to_orphan_coverage(repo_root, &facts, snapshot);
    let py_refs: Vec<&kiss::ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&kiss::ParsedRustFile> = rs_parsed.iter().collect();
    let py_ctx = if py_parsed.is_empty() {
        kiss::ContextDependencyGraph::empty()
    } else {
        build_python_context_graph(&py_refs, &roles)
    };
    let rs_ctx = if rs_parsed.is_empty() {
        kiss::ContextDependencyGraph::empty()
    } else {
        build_rust_context_graph(&rs_refs, &roles)
    };
    let py_prod = py_ctx.production_view();
    let rs_prod = rs_ctx.production_view();
    let entries = collect_orphan_entry_paths(
        &py_parsed,
        &rs_parsed,
        (!py_parsed.is_empty()).then_some(&py_prod),
        (!rs_parsed.is_empty()).then_some(&rs_prod),
    );
    let viols = orphan_unit_violations(&OrphanUnitInput {
        py: &py_parsed,
        rs: &rs_parsed,
        py_ctx: &py_ctx,
        rs_ctx: &rs_ctx,
        entries: &entries,
        orphan_allowed: &gate.orphan_allowed,
        repo_root,
        roles: &roles,
        coverage: Some(&coverage),
    });
    kiss::cli_output::print_violations(&viols);
    !viols.is_empty()
}

fn snapshot_to_orphan_coverage(
    repo_root: &Path,
    facts: &CoverageSourceFacts,
    snapshot: &RuntimeCoverageSnapshot,
) -> OrphanCoverage {
    let mut hit = std::collections::BTreeMap::new();
    for path in facts.coverable_map().keys() {
        let lines = repo_relative_key(repo_root, path)
            .and_then(|key| snapshot.covered_lines.get(&key))
            .map(|set| set.iter().map(|n| *n as usize).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        hit.insert(path.clone(), lines);
    }
    OrphanCoverage {
        coverable: facts.coverable_map().clone(),
        hit,
    }
}

#[cfg(test)]
mod orphan_unit_gate_test {
    use super::{evaluate_orphan_unit_gate, snapshot_to_orphan_coverage};
    use crate::analyze::line_coverage::{CoverageSourceFacts, RuntimeCoverageSnapshot};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    fn snap(lines: BTreeMap<String, BTreeSet<u32>>) -> RuntimeCoverageSnapshot {
        RuntimeCoverageSnapshot {
            identity: "id".into(),
            covered_lines: lines,
        }
    }

    fn enabled_gate() -> kiss::GateConfig {
        kiss::GateConfig {
            orphan_unit_enabled: true,
            ..kiss::GateConfig::default()
        }
    }

    #[test]
    fn snapshot_maps_relative_hits() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("a.py");
        std::fs::write(&file, "x = 1\n").unwrap();
        let facts = CoverageSourceFacts::from_files(std::slice::from_ref(&file), &[]).unwrap();
        let cov = snapshot_to_orphan_coverage(
            tmp.path(),
            &facts,
            &snap(BTreeMap::from([("a.py".into(), BTreeSet::from([1]))])),
        );
        assert!(cov.hit.get(&file).is_some_and(|h| h.contains(&1)));
    }

    #[test]
    fn bypass_or_disabled_is_not_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let empty = snap(BTreeMap::new());
        let disabled = kiss::GateConfig::default();
        assert!(!evaluate_orphan_unit_gate(
            tmp.path(),
            &[],
            &[],
            &empty,
            &disabled,
            false
        ));
        assert!(!evaluate_orphan_unit_gate(
            tmp.path(),
            &[],
            &[],
            &empty,
            &enabled_gate(),
            true
        ));
    }

    #[test]
    fn unused_python_helper_fails_when_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let utils = tmp.path().join("utils.py");
        std::fs::write(&utils, "def helper():\n    return 1\n").unwrap();
        let failed = evaluate_orphan_unit_gate(
            tmp.path(),
            &[utils],
            &[],
            &snap(BTreeMap::from([(
                "utils.py".into(),
                BTreeSet::from([1]),
            )])),
            &enabled_gate(),
            false,
        );
        assert!(failed);
    }

    #[test]
    fn body_hit_clears_python_helper() {
        let tmp = tempfile::TempDir::new().unwrap();
        let utils = tmp.path().join("utils.py");
        std::fs::write(&utils, "x = 1\ndef helper():\n    return 1\n").unwrap();
        let failed = evaluate_orphan_unit_gate(
            tmp.path(),
            &[utils],
            &[],
            &snap(BTreeMap::from([(
                "utils.py".into(),
                BTreeSet::from([1, 2, 3]),
            )])),
            &enabled_gate(),
            false,
        );
        assert!(!failed);
    }

    #[test]
    fn unused_rust_fn_fails_when_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let lib = tmp.path().join("lib.rs");
        std::fs::write(&lib, "pub fn unused() { let _x = 1; }\n").unwrap();
        let failed = evaluate_orphan_unit_gate(
            tmp.path(),
            &[],
            &[lib],
            &snap(BTreeMap::new()),
            &enabled_gate(),
            false,
        );
        assert!(failed);
    }

    #[test]
    fn parse_failure_fails_closed() {
        let missing = PathBuf::from("/no/such/orphan_unit_gate_missing.py");
        assert!(evaluate_orphan_unit_gate(
            PathBuf::from("/tmp").as_path(),
            &[missing],
            &[],
            &snap(BTreeMap::new()),
            &enabled_gate(),
            false,
        ));
    }
}
