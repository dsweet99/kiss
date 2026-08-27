use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::code_roles::build_source_role_index;
use crate::graph::{
    build_python_context_graph, collect_orphan_entry_paths, orphan_unit_violations, OrphanCoverage,
    OrphanUnitInput,
};
use crate::parsing::{create_parser, parse_file, ParsedFile};
use crate::rust_graph::build_rust_context_graph;
use crate::rust_parsing::{parse_rust_file, ParsedRustFile};

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser");
    parse_file(&mut parser, path).expect("parse python")
}

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust")
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn py_names(files: &[PathBuf], coverage: Option<&OrphanCoverage>, root: &Path) -> Vec<String> {
    let parsed: Vec<ParsedFile> = files.iter().map(|p| parse_py(p)).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let roles = build_source_role_index(&parsed, &[], files, &[]).unwrap();
    let ctx = build_python_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&parsed, &[], Some(&prod), None);
    let empty_rs = [];
    let empty_rs_ctx = crate::graph::ContextDependencyGraph::empty();
    orphan_unit_violations(&OrphanUnitInput {
        py: &parsed,
        rs: &empty_rs,
        py_ctx: &ctx,
        rs_ctx: &empty_rs_ctx,
        entries: &entries,
        orphan_allowed: &[],
        repo_root: root,
        roles: &roles,
        coverage,
    })
    .into_iter()
    .map(|v| v.unit_name)
    .collect()
}

fn cov(path: &Path, coverable: &[usize], hit: &[usize]) -> OrphanCoverage {
    let mut coverable_map = BTreeMap::new();
    let mut hit_map = BTreeMap::new();
    coverable_map.insert(path.to_path_buf(), coverable.iter().copied().collect());
    hit_map.insert(path.to_path_buf(), hit.iter().copied().collect());
    OrphanCoverage {
        coverable: coverable_map,
        hit: hit_map,
    }
}

fn multi_cov(files: &[(PathBuf, Vec<usize>, Vec<usize>)]) -> OrphanCoverage {
    let mut coverable = BTreeMap::new();
    let mut hit = BTreeMap::new();
    for (path, c, h) in files {
        coverable.insert(path.clone(), c.iter().copied().collect());
        hit.insert(path.clone(), h.iter().copied().collect());
    }
    OrphanCoverage { coverable, hit }
}

#[test]
fn missing_coverage_is_unevaluated() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(&utils, "def helper():\n    return 1\n");
    let names = py_names(&[utils], None, tmp.path());
    assert!(names.is_empty(), "missing snapshot must not emit: {names:?}");
}

#[test]
fn unused_helper_in_imported_module_is_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_u.py");
    write(&utils, "def helper():\n    return 1\n");
    write(
        &test,
        "import utils\n\ndef test_import():\n    assert True\n",
    );
    let coverage = multi_cov(&[
        (utils.clone(), vec![1, 2], vec![1]),
        (test.clone(), vec![1], vec![1]),
    ]);
    let names = py_names(&[utils, test], Some(&coverage), tmp.path());
    assert!(
        names.iter().any(|n| n == "helper"),
        "unused helper must be orphan: {names:?}"
    );
}

#[test]
fn named_import_graph_witnesses_helper() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_u.py");
    write(&utils, "def helper():\n    return 1\n");
    write(
        &test,
        "from utils import helper\n\ndef test_h():\n    assert helper() == 1\n",
    );
    let coverage = multi_cov(&[
        (utils.clone(), vec![1, 2], vec![1]),
        (test.clone(), vec![1, 4], vec![1, 4]),
    ]);
    let names = py_names(&[utils, test], Some(&coverage), tmp.path());
    assert!(
        !names.iter().any(|n| n == "helper"),
        "named import must clear helper: {names:?}"
    );
}

#[test]
fn body_hit_is_coverage_witness() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_u.py");
    write(&utils, "def helper():\n    return 1\n");
    write(&test, "import utils\n");
    let coverage = multi_cov(&[
        (utils.clone(), vec![1, 2], vec![1, 2]),
        (test.clone(), vec![1], vec![1]),
    ]);
    let names = py_names(&[utils, test], Some(&coverage), tmp.path());
    assert!(
        !names.iter().any(|n| n == "helper"),
        "body hit must clear helper: {names:?}"
    );
}

#[test]
fn def_line_only_is_not_coverage_witness() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(&utils, "def helper():\n    return 1\n");
    let coverage = cov(&utils, &[1, 2], &[1]);
    let names = py_names(&[utils], Some(&coverage), tmp.path());
    assert!(
        names.iter().any(|n| n == "helper" || n == "utils.py"),
        "def-only hit must leave helper unused: {names:?}"
    );
}

#[test]
fn test_only_file_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let test = tmp.path().join("tests").join("test_only.py");
    write(&test, "def test_x():\n    assert True\n");
    let coverage = cov(&test, &[1, 2], &[]);
    let names = py_names(&[test], Some(&coverage), tmp.path());
    assert!(names.is_empty(), "test-only must not be reported: {names:?}");
}

#[test]
fn file_collapses_when_every_candidate_is_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lonely = tmp.path().join("lonely.py");
    write(&lonely, "def helper():\n    return 1\n");
    let coverage = cov(&lonely, &[1, 2], &[]);
    let names = py_names(&[lonely], Some(&coverage), tmp.path());
    assert_eq!(names, vec!["lonely.py".to_string()]);
}

#[test]
fn rust_use_names_helper_type() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(&src.join("lib.rs"), "mod m;\nuse crate::m::Helper;\n");
    write(
        &src.join("m.rs"),
        "pub struct Helper;\npub fn unused() { let _x = 1; }\n",
    );
    let files = vec![src.join("lib.rs"), src.join("m.rs")];
    let parsed: Vec<ParsedRustFile> = files.iter().map(|p| parse_rs(p)).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let roles = build_source_role_index(&[], &parsed, &[], &files).unwrap();
    let ctx = build_rust_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&[], &parsed, None, Some(&prod));
    let m = src.join("m.rs");
    let coverage = OrphanCoverage {
        coverable: BTreeMap::from([(m.clone(), BTreeSet::from([2]))]),
        hit: BTreeMap::from([(m, BTreeSet::new())]),
    };
    let empty_py = [];
    let empty_py_ctx = crate::graph::ContextDependencyGraph::empty();
    let names: Vec<String> = orphan_unit_violations(&OrphanUnitInput {
        py: &empty_py,
        rs: &parsed,
        py_ctx: &empty_py_ctx,
        rs_ctx: &ctx,
        entries: &entries,
        orphan_allowed: &[],
        repo_root: tmp.path(),
        roles: &roles,
        coverage: Some(&coverage),
    })
    .into_iter()
    .map(|v| v.unit_name)
    .collect();
    assert!(
        names.iter().any(|n| n == "unused"),
        "unused rust fn must be orphan: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "Helper"),
        "named Helper must not be orphan: {names:?}"
    );
}

#[test]
fn rust_fn_main_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("src/main.rs"),
        "fn helper() { let _x = 1; }\nfn main() {}\n",
    );
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let main = tmp.path().join("src/main.rs");
    let files = vec![main.clone()];
    let parsed: Vec<ParsedRustFile> = files.iter().map(|p| parse_rs(p)).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let roles = build_source_role_index(&[], &parsed, &[], &files).unwrap();
    let ctx = build_rust_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&[], &parsed, None, Some(&prod));
    let coverage = OrphanCoverage {
        coverable: BTreeMap::from([(main.clone(), BTreeSet::from([1, 2]))]),
        hit: BTreeMap::from([(main, BTreeSet::new())]),
    };
    let empty_py = [];
    let empty_py_ctx = crate::graph::ContextDependencyGraph::empty();
    let names: HashSet<String> = orphan_unit_violations(&OrphanUnitInput {
        py: &empty_py,
        rs: &parsed,
        py_ctx: &empty_py_ctx,
        rs_ctx: &ctx,
        entries: &entries,
        orphan_allowed: &[],
        repo_root: tmp.path(),
        roles: &roles,
        coverage: Some(&coverage),
    })
    .into_iter()
    .map(|v| v.unit_name)
    .collect();
    assert!(
        !names.iter().any(|n| n == "main"),
        "fn main must not be a candidate: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "helper" || n == "main.rs"),
        "other fn in entry file remains a candidate: {names:?}"
    );
}

#[test]
fn decorator_line_does_not_witness_function() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(
        &utils,
        "def dec(f):\n    return f\n\n@dec\ndef helper():\n    return 1\n",
    );
    let coverage = cov(&utils, &[1, 2, 4, 5, 6], &[4]);
    let names = py_names(&[utils], Some(&coverage), tmp.path());
    assert!(
        names.iter().any(|n| n == "helper" || n == "utils.py"),
        "decorator hit must not clear helper: {names:?}"
    );
}

#[test]
fn main_guard_module_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let run = tmp.path().join("run.py");
    write(
        &run,
        "def used():\n    return 1\n\ndef helper():\n    return 2\n\nif __name__ == \"__main__\":\n    pass\n",
    );
    let coverage = cov(&run, &[1, 2, 4, 5, 7, 8], &[2]);
    let names = py_names(&[run], Some(&coverage), tmp.path());
    assert!(
        !names.iter().any(|n| n == "run" || n == "run.py"),
        "main-guard module must not be a candidate: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "helper"),
        "nested unit in entry file remains a candidate: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "used"),
        "covered nested unit must not be orphan: {names:?}"
    );
}
