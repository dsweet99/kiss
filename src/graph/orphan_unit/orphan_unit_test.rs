use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::code_roles::build_source_role_index;
use crate::graph::{
    OrphanUnitInput, build_python_context_graph, collect_orphan_entry_callables,
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

fn py_names(files: &[PathBuf], root: &Path) -> Vec<String> {
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
    })
    .into_iter()
    .map(|v| v.unit_name)
    .collect()
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
    let names = py_names(&[utils, test], tmp.path());
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
    let names = py_names(&[utils, test], tmp.path());
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
    let names = py_names(&[utils, test], tmp.path());
    assert!(
        !names.iter().any(|n| n == "helper"),
        "named import must clear helper: {names:?}"
    );
}

#[test]
fn eval_fstring_import_does_not_orphan_pytest_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let ops = tmp.path().join("evaluate.py");
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_u.py");
    write(
        &ops,
        "import importlib\n\
         def run(group, eval_name):\n    \
         importlib.import_module(f\"evals.{group}.eval_{eval_name}\")\n",
    );
    write(&utils, "def helper():\n    return 1\n");
    write(
        &test,
        "from utils import helper\n\ndef test_h():\n    assert helper() == 1\n",
    );
    let names = py_names(&[ops, utils, test], tmp.path());
    assert!(
        !names
            .iter()
            .any(|name| name == "helper" || name == "test_h"),
        "prefixed eval import must not orphan the pytest test or its import: {names:?}"
    );
}

#[test]
fn test_only_file_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let test = tmp.path().join("tests").join("test_only.py");
    write(&test, "def test_x():\n    assert True\n");
    let names = py_names(&[test], tmp.path());
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
    let names = py_names(&[lonely], tmp.path());
    assert_eq!(names, vec!["lonely.py".to_string()]);
}

#[test]
fn rust_use_names_helper_type() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "mod m;\nuse crate::m::Helper;\nfn f() { let _ = Helper; }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        super::f();\n    }\n}\n",
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
fn main_guard_module_is_not_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let run = tmp.path().join("run.py");
    write(
        &run,
        "def used():\n    return 1\n\ndef helper():\n    return 2\n\nif __name__ == \"__main__\":\n    used()\n",
    );
    let names = py_names(&[run], tmp.path());
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
        "unit called from the main guard must not be orphan: {names:?}"
    );
}

fn rust_names(files: &[PathBuf], root: &Path) -> Vec<String> {
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
        "mod m;\nfn f() { let _ = crate::m::Helper; }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        super::f();\n    }\n}\n",
    );
    write(
        &src.join("m.rs"),
        "pub struct Helper;\npub fn unused() { let _x = 1; }\n",
    );
    let files = vec![src.join("lib.rs"), src.join("m.rs")];
    let names = rust_names(&files, tmp.path());
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
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
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
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
    assert!(
        !names.iter().any(|n| n == "default"),
        "trait impl method must not be a candidate: {names:?}"
    );
}

#[test]
fn rust_trait_impl_method_roots_its_callees() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("lib.rs"),
        "pub struct Helper;\nimpl Default for Helper { fn default() -> Self { called(); Helper } }\nfn called() { let _x = 1; }\nfn unused() { let _x = 2; }\n",
    );
    let lib = src.join("lib.rs");
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
    assert!(
        !names.iter().any(|n| n == "called"),
        "callee of a trait impl method must not be orphan: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "unused"),
        "unused rust fn must be orphan: {names:?}"
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
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
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
        "mod m;\nfn f() { crate::m::used(); }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        super::f();\n    }\n}\n",
    );
    write(
        &src.join("m/mod.rs"),
        "pub fn used() { let _x = 1; }\npub fn unused() { let _x = 2; }\n",
    );
    let files = vec![src.join("lib.rs"), src.join("m/mod.rs")];
    let names = rust_names(&files, tmp.path());
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
        "pub struct Unused;\npub struct Used;\nfn f() { let _ = Used; }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        super::f();\n    }\n}\n",
    );
    let lib = tmp.path().join("src/lib.rs");
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
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
    let names = py_names(&[cli.clone(), pkg.join("__init__.py")], tmp.path());
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
    let names = rust_names(std::slice::from_ref(&lib), tmp.path());
    assert!(
        !names.is_empty(),
        "unreached cargo lib units must be reportable: {names:?}"
    );
}

#[test]
fn rust_reexported_fn_reaches_its_module() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("main.rs"),
        "mod m;\nmod idle;\nfn main() {\n    m::helper(1);\n}\n",
    );
    write(
        &src.join("m/mod.rs"),
        "mod planning;\npub(crate) use planning::helper;\n",
    );
    write(
        &src.join("m/planning.rs"),
        "pub(crate) fn helper(x: u8) -> u8 {\n    x + 1\n}\n",
    );
    write(&src.join("idle.rs"), "fn unused() {\n    let _x = 1;\n}\n");
    let files = vec![
        src.join("main.rs"),
        src.join("m/mod.rs"),
        src.join("m/planning.rs"),
        src.join("idle.rs"),
    ];
    let names = rust_names(&files, tmp.path());
    assert_eq!(names, ["idle.rs"]);
}

#[test]
fn rust_mod_rs_module_is_reached_by_its_directory_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(
        &src.join("main.rs"),
        "mod fc;\nmod idle;\nfn main() {\n    let _ = fc::S;\n}\n",
    );
    write(
        &src.join("fc/mod.rs"),
        "mod part;\npub static S: &str = part::A;\n",
    );
    write(
        &src.join("fc/part.rs"),
        "pub(crate) const A: &str = \"a\";\n",
    );
    write(&src.join("idle.rs"), "pub(crate) const B: &str = \"b\";\n");
    let files = vec![
        src.join("main.rs"),
        src.join("fc/mod.rs"),
        src.join("fc/part.rs"),
        src.join("idle.rs"),
    ];
    let names = rust_names(&files, tmp.path());
    assert_eq!(names, ["idle.rs"]);
}
