use std::path::{Path, PathBuf};

use kiss::{
    OrphanUnitInput, build_python_context_graph, build_rust_context_graph,
    collect_orphan_entry_callables, collect_orphan_entry_paths, orphan_unit_findings,
};

use crate::analyze_parse::parse_classified;

pub(crate) fn collect_orphan_unit_findings(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    orphan_allowed: &[String],
) -> Result<Vec<kiss::OrphanUnitFinding>, ()> {
    with_orphan_input(
        repo_root,
        py_files,
        rs_files,
        orphan_allowed,
        orphan_unit_findings,
    )
}

fn with_orphan_input<T>(
    repo_root: &Path,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    orphan_allowed: &[String],
    finish: impl FnOnce(&OrphanUnitInput<'_>) -> T,
) -> Result<T, ()> {
    let Ok((py_parsed, rs_parsed, roles)) = parse_classified(py_files, rs_files) else {
        return Err(());
    };
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
    let py_graph = (!py_parsed.is_empty()).then_some(&py_prod);
    let rs_graph = (!rs_parsed.is_empty()).then_some(&rs_prod);
    let entries = collect_orphan_entry_paths(&py_parsed, &rs_parsed, py_graph, rs_graph);
    let callables = collect_orphan_entry_callables(&py_parsed, &rs_parsed, py_graph, rs_graph);
    Ok(finish(&OrphanUnitInput {
        py: &py_parsed,
        rs: &rs_parsed,
        py_ctx: &py_ctx,
        rs_ctx: &rs_ctx,
        entries: &entries,
        entry_callables: &callables,
        orphan_allowed,
        repo_root,
        roles: &roles,
    }))
}

#[cfg(test)]
mod orphan_unit_gate_test {
    use super::collect_orphan_unit_findings;
    use std::path::PathBuf;

    fn has_orphans(repo_root: &std::path::Path, py: &[PathBuf], rs: &[PathBuf]) -> bool {
        collect_orphan_unit_findings(repo_root, py, rs, &[]).map_or(true, |found| !found.is_empty())
    }

    #[test]
    fn unused_python_helper_fails_when_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let utils = tmp.path().join("utils.py");
        std::fs::write(&utils, "def helper():\n    return 1\n").unwrap();
        assert!(has_orphans(tmp.path(), &[utils], &[]));
    }

    #[test]
    fn test_reference_clears_python_helper() {
        let tmp = tempfile::TempDir::new().unwrap();
        let utils = tmp.path().join("utils.py");
        std::fs::write(&utils, "def helper():\n    return 1\n").unwrap();
        let test = tmp.path().join("test_utils.py");
        std::fs::write(
            &test,
            "from utils import helper\n\ndef test_helper():\n    assert helper() == 1\n",
        )
        .unwrap();
        assert!(!has_orphans(tmp.path(), &[utils, test], &[]));
    }

    #[test]
    fn unused_rust_fn_fails_when_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let lib = tmp.path().join("lib.rs");
        std::fs::write(&lib, "pub fn unused() { let _x = 1; }\n").unwrap();
        assert!(has_orphans(tmp.path(), &[], &[lib]));
    }

    #[test]
    fn parse_failure_fails_closed() {
        let missing = PathBuf::from("/no/such/orphan_unit_gate_missing.py");
        assert!(has_orphans(
            PathBuf::from("/tmp").as_path(),
            &[missing],
            &[]
        ));
    }
}
