use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::code_roles::build_source_role_index;
use crate::graph::{
    OrphanCoverage, OrphanUnitInput, build_python_context_graph, collect_orphan_entry_callables,
    collect_orphan_entry_paths, orphan_unit_violations,
};
use crate::parsing::{ParsedFile, create_parser, parse_file};
use crate::rust_graph::build_rust_context_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_file};

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
    let callables = collect_orphan_entry_callables(&parsed, &[], Some(&prod), None);
    let empty_rs = [];
    let empty_rs_ctx = crate::graph::ContextDependencyGraph::empty();
    orphan_unit_violations(&OrphanUnitInput {
        py: &parsed,
        rs: &empty_rs,
        py_ctx: &ctx,
        rs_ctx: &empty_rs_ctx,
        entries: &entries,
        entry_callables: &callables,
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
    assert!(
        names.is_empty(),
        "missing snapshot must not emit: {names:?}"
    );
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
fn nested_name_is_not_an_edge_of_the_container() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_u.py");
    write(
        &utils,
        "def helper():\n    return 1\n\ndef outer():\n    def inner():\n        return helper()\n    return 2\n",
    );
    write(
        &test,
        "import utils\n\ndef test_outer():\n    assert utils.outer() == 2\n",
    );
    let coverage = multi_cov(&[
        (utils.clone(), vec![1, 2, 4, 5, 6, 7], vec![4, 7]),
        (test.clone(), vec![1, 3, 4], vec![1, 3, 4]),
    ]);
    let names = py_names(&[utils, test], Some(&coverage), tmp.path());
    assert!(
        names.iter().any(|n| n == "helper"),
        "name inside unreached inner must not clear helper: {names:?}"
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
    assert!(
        names.is_empty(),
        "test-only must not be reported: {names:?}"
    );
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
    write(
        &src.join("lib.rs"),
        "mod m;\nuse crate::m::Helper;\nfn f() { let _ = Helper; }\n",
    );
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
    let callables = collect_orphan_entry_callables(&[], &parsed, None, Some(&prod));
    let lib = src.join("lib.rs");
    let m = src.join("m.rs");
    let coverage = OrphanCoverage {
        coverable: BTreeMap::from([
            (lib.clone(), BTreeSet::from([3])),
            (m.clone(), BTreeSet::from([2])),
        ]),
        hit: BTreeMap::from([(lib, BTreeSet::from([3])), (m, BTreeSet::new())]),
    };
    let empty_py = [];
    let empty_py_ctx = crate::graph::ContextDependencyGraph::empty();
    let names: Vec<String> = orphan_unit_violations(&OrphanUnitInput {
        py: &empty_py,
        rs: &parsed,
        py_ctx: &empty_py_ctx,
        rs_ctx: &ctx,
        entries: &entries,
        entry_callables: &callables,
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
    let callables = collect_orphan_entry_callables(&[], &parsed, None, Some(&prod));
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
        entry_callables: &callables,
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

fn rust_names(files: &[PathBuf], coverage: OrphanCoverage, root: &Path) -> Vec<String> {
    let parsed: Vec<ParsedRustFile> = files.iter().map(|p| parse_rs(p)).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let roles = build_source_role_index(&[], &parsed, &[], files).unwrap();
    let ctx = build_rust_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&[], &parsed, None, Some(&prod));
    let callables = collect_orphan_entry_callables(&[], &parsed, None, Some(&prod));
    let empty_py = [];
    let empty_py_ctx = crate::graph::ContextDependencyGraph::empty();
    orphan_unit_violations(&OrphanUnitInput {
        py: &empty_py,
        rs: &parsed,
        py_ctx: &empty_py_ctx,
        rs_ctx: &ctx,
        entries: &entries,
        entry_callables: &callables,
        orphan_allowed: &[],
        repo_root: root,
        roles: &roles,
        coverage: Some(&coverage),
    })
    .into_iter()
    .map(|v| v.unit_name)
    .collect()
}

#[test]
fn rust_path_expr_names_helper_type() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "mod m;\nfn f() { let _ = crate::m::Helper; }\n",
    );
    write(
        &src.join("m.rs"),
        "pub struct Helper;\npub fn unused() { let _x = 1; }\n",
    );
    let files = vec![src.join("lib.rs"), src.join("m.rs")];
    let lib = src.join("lib.rs");
    let m = src.join("m.rs");
    let names = rust_names(
        &files,
        OrphanCoverage {
            coverable: BTreeMap::from([
                (lib.clone(), BTreeSet::from([2])),
                (m.clone(), BTreeSet::from([2])),
            ]),
            hit: BTreeMap::from([(lib, BTreeSet::from([2])), (m, BTreeSet::new())]),
        },
        tmp.path(),
    );
    assert!(
        names.iter().any(|n| n == "unused"),
        "unused rust fn must be orphan: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "Helper"),
        "path-named Helper must not be orphan: {names:?}"
    );
}

#[test]
fn rust_same_module_type_name_witnesses_struct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "pub struct Helper;\nfn f() { let _ = Helper; }\n",
    );
    let lib = src.join("lib.rs");
    let names = rust_names(
        std::slice::from_ref(&lib),
        OrphanCoverage {
            coverable: BTreeMap::from([(lib.clone(), BTreeSet::from([2]))]),
            hit: BTreeMap::from([(lib.clone(), BTreeSet::new())]),
        },
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "Helper"),
        "same-module Helper must not be orphan: {names:?}"
    );
}

#[test]
fn rust_trait_impl_method_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "pub struct Helper;\nimpl Default for Helper { fn default() -> Self { Helper } }\n",
    );
    let lib = src.join("lib.rs");
    let names = rust_names(
        std::slice::from_ref(&lib),
        OrphanCoverage {
            coverable: BTreeMap::from([(lib.clone(), BTreeSet::from([2]))]),
            hit: BTreeMap::from([(lib.clone(), BTreeSet::new())]),
        },
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "default"),
        "trait impl method must not be a candidate: {names:?}"
    );
}

#[test]
fn rust_enum_variant_path_witnesses_type() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "pub enum Helper { A }\nfn f() { let _ = Helper::A; }\n",
    );
    let lib = src.join("lib.rs");
    let names = rust_names(
        std::slice::from_ref(&lib),
        OrphanCoverage {
            coverable: BTreeMap::from([(lib.clone(), BTreeSet::from([2]))]),
            hit: BTreeMap::from([(lib.clone(), BTreeSet::from([2]))]),
        },
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "Helper"),
        "Helper::A must witness Helper: {names:?}"
    );
}

#[test]
fn rust_mod_rs_module_unit_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "mod m;\nfn f() { crate::m::used(); }\n",
    );
    write(
        &src.join("m/mod.rs"),
        "pub fn used() { let _x = 1; }\npub fn unused() { let _x = 2; }\n",
    );
    let files = vec![src.join("lib.rs"), src.join("m/mod.rs")];
    let lib = src.join("lib.rs");
    let m = src.join("m/mod.rs");
    let names = rust_names(
        &files,
        OrphanCoverage {
            coverable: BTreeMap::from([
                (lib.clone(), BTreeSet::from([2])),
                (m.clone(), BTreeSet::from([1, 2])),
            ]),
            hit: BTreeMap::from([(lib, BTreeSet::from([2])), (m, BTreeSet::from([1]))]),
        },
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "mod" || n == "mod.rs"),
        "mod.rs module unit must not be a candidate: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "unused"),
        "nested unit in mod.rs remains a candidate: {names:?}"
    );
}

#[test]
fn rust_cargo_lib_module_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &tmp.path().join("src/lib.rs"),
        "pub struct Unused;\npub struct Used;\nfn f() { let _ = Used; }\n",
    );
    let lib = tmp.path().join("src/lib.rs");
    let names = rust_names(
        std::slice::from_ref(&lib),
        OrphanCoverage {
            coverable: BTreeMap::from([(lib.clone(), BTreeSet::from([3]))]),
            hit: BTreeMap::from([(lib.clone(), BTreeSet::from([3]))]),
        },
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "lib" || n == "lib.rs"),
        "cargo lib module must not be a candidate: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "Unused"),
        "unused struct in lib remains a candidate: {names:?}"
    );
}

#[test]
fn exclusive_class_body_line_roots_class() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(
        &utils,
        "class C:\n    x = 1\n    def m(self):\n        return 1\n",
    );
    let names = py_names(
        std::slice::from_ref(&utils),
        Some(&cov(&utils, &[2, 3, 4], &[2])),
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "C"),
        "exclusive class-body hit must root the class: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "m"),
        "unreached method remains a candidate: {names:?}"
    );
}

#[test]
fn python_script_callable_is_root() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("pyproject.toml"),
        "[project]\nname = \"d\"\nversion = \"0.1.0\"\n[project.scripts]\ncli = \"pkg.cli:main\"\n",
    );
    let pkg = tmp.path().join("pkg");
    write(&pkg.join("__init__.py"), "");
    write(
        &pkg.join("cli.py"),
        "def main():\n    return 1\n\ndef helper():\n    return 2\n",
    );
    let cli = pkg.join("cli.py");
    let names = py_names(
        &[cli.clone(), pkg.join("__init__.py")],
        Some(&cov(&cli, &[1, 2, 4, 5], &[])),
        tmp.path(),
    );
    assert!(
        !names.iter().any(|n| n == "main"),
        "script callable main must not be a finding: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "helper" || n == "cli.py"),
        "other unit in the script file remains a candidate: {names:?}"
    );
}

#[test]
fn unused_lib_rs_module_can_be_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&tmp.path().join("src/lib.rs"), "pub struct Unused;\n");
    let lib = tmp.path().join("src/lib.rs");
    let names = rust_names(
        std::slice::from_ref(&lib),
        OrphanCoverage {
            coverable: BTreeMap::from([(lib.clone(), BTreeSet::from([1]))]),
            hit: BTreeMap::from([(lib.clone(), BTreeSet::new())]),
        },
        tmp.path(),
    );
    assert!(
        !names.is_empty(),
        "unreached cargo lib units must be reportable: {names:?}"
    );
}
