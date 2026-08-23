use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::code_roles::build_source_role_index;
use crate::graph::{
    analyze_graph, build_python_context_graph, collect_orphan_entry_paths, orphan_violations,
};
use crate::parsing::{ParsedFile, create_parser, parse_file};
use crate::rust_graph::build_rust_context_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_file};
use crate::Config;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser");
    parse_file(&mut parser, path).expect("parse python")
}

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust")
}

fn py_report(
    files: &[PathBuf],
    orphan_allowed: &[String],
    repo_root: &Path,
) -> (Vec<crate::Violation>, HashSet<PathBuf>) {
    let parsed: Vec<ParsedFile> = files.iter().map(|p| parse_py(p)).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let roles = build_source_role_index(&parsed, &[], files, &[]).unwrap();
    let ctx = build_python_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&parsed, &[], Some(&prod), None);
    (
        orphan_violations(&ctx, &prod, &entries, orphan_allowed, repo_root),
        entries,
    )
}

fn rs_report(
    files: &[PathBuf],
    orphan_allowed: &[String],
    repo_root: &Path,
) -> Vec<crate::Violation> {
    let parsed: Vec<ParsedRustFile> = files.iter().map(|p| parse_rs(p)).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let roles = build_source_role_index(&[], &parsed, &[], files).unwrap();
    let ctx = build_rust_context_graph(&refs, &roles);
    let prod = ctx.production_view();
    let entries = collect_orphan_entry_paths(&[], &parsed, None, Some(&prod));
    orphan_violations(&ctx, &prod, &entries, orphan_allowed, repo_root)
}

fn orphan_names(viols: &[crate::Violation]) -> Vec<String> {
    viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .map(|v| v.unit_name.clone())
        .collect()
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn python_test_import_clears_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    let test = tmp.path().join("tests").join("test_foo.py");
    write(&utils, "def f():\n    return 1\n");
    write(&test, "from utils import f\n\ndef test_f():\n    assert f() == 1\n");
    let (viols, _) = py_report(&[utils, test], &[], tmp.path());
    assert!(
        orphan_names(&viols).is_empty(),
        "test-imported utils must not be orphan: {viols:#?}"
    );
}

#[test]
fn production_isolate_is_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(&utils, "def f():\n    return 1\n");
    let (viols, _) = py_report(&[utils], &[], tmp.path());
    assert!(
        orphan_names(&viols).iter().any(|n| n.ends_with("utils")),
        "isolated utils.py must be orphan: {viols:#?}"
    );
}

#[test]
fn test_only_file_is_not_orphan_candidate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let test = tmp.path().join("tests").join("test_only.py");
    write(&test, "def test_x():\n    assert True\n");
    let (viols, _) = py_report(&[test], &[], tmp.path());
    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "test-only file must not be reported: {viols:#?}"
    );
}

#[test]
fn rust_cfg_test_use_clears_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    write(&src.join("lib.rs"), "mod mixed;\n");
    write(&src.join("helper.rs"), "pub fn f() {}\n");
    write(
        &src.join("mixed.rs"),
        "#[cfg(test)]\nmod tests {\n    use helper;\n}\n",
    );
    let files = vec![src.join("lib.rs"), src.join("helper.rs"), src.join("mixed.rs")];
    let viols = rs_report(&files, &[], tmp.path());
    assert!(
        !orphan_names(&viols).iter().any(|n| n.contains("helper")),
        "cfg(test) use must clear helper: {viols:#?}"
    );
}

#[test]
fn rust_tests_dir_use_clears_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), "pub fn f() {}\n");
    write(&tmp.path().join("src/helper.rs"), "pub fn g() {}\n");
    write(&tmp.path().join("tests/uses_helper.rs"), "use helper;\n");
    let files = vec![
        tmp.path().join("src/lib.rs"),
        tmp.path().join("src/helper.rs"),
        tmp.path().join("tests/uses_helper.rs"),
    ];
    let viols = rs_report(&files, &[], tmp.path());
    assert!(
        !orphan_names(&viols).iter().any(|n| n.contains("helper")),
        "tests/*.rs use must clear helper: {viols:#?}"
    );
}

#[test]
fn rust_isolate_is_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), "pub fn f() {}\n");
    write(&tmp.path().join("src/lonely.rs"), "pub fn g() {}\n");
    let files = vec![tmp.path().join("src/lib.rs"), tmp.path().join("src/lonely.rs")];
    let viols = rs_report(&files, &[], tmp.path());
    assert!(
        orphan_names(&viols).iter().any(|n| n.contains("lonely")),
        "lonely.rs must be orphan: {viols:#?}"
    );
}

#[test]
fn python_main_guard_is_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let run = tmp.path().join("scripts").join("run.py");
    write(&run, "if __name__ == \"__main__\":\n    print(1)\n");
    let (viols, entries) = py_report(std::slice::from_ref(&run), &[], tmp.path());
    assert!(
        entries.iter().any(|p| p.ends_with("run.py")),
        "main guard must add entry path: {entries:?}"
    );
    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "scripts/run.py with main guard must not be orphan: {viols:#?}"
    );
}

#[test]
fn pyproject_scripts_are_entries() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("pyproject.toml"),
        "[project]\nname = \"d\"\nversion = \"0\"\n[project.scripts]\ntool = \"pkg.cli:main\"\n",
    );
    write(&tmp.path().join("pkg/__init__.py"), "");
    write(&tmp.path().join("pkg/cli.py"), "def main():\n    return 0\n");
    let files = vec![
        tmp.path().join("pkg/__init__.py"),
        tmp.path().join("pkg/cli.py"),
    ];
    let (viols, _) = py_report(&files, &[], tmp.path());
    assert!(
        !orphan_names(&viols).iter().any(|n| n.ends_with("cli")),
        "project.scripts module must not be orphan: {viols:#?}"
    );
}

#[test]
fn pyproject_gui_scripts_are_entries() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("pyproject.toml"),
        "[project]\nname = \"d\"\nversion = \"0\"\n[project.gui-scripts]\napp = \"pkg.ui:run\"\n",
    );
    write(&tmp.path().join("pkg/__init__.py"), "");
    write(&tmp.path().join("pkg/ui.py"), "def run():\n    return 0\n");
    let files = vec![tmp.path().join("pkg/__init__.py"), tmp.path().join("pkg/ui.py")];
    let (viols, _) = py_report(&files, &[], tmp.path());
    assert!(
        !orphan_names(&viols).iter().any(|n| n.ends_with("ui")),
        "gui-scripts module must not be orphan: {viols:#?}"
    );
}

#[test]
fn setup_cfg_console_scripts_are_entries() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("setup.cfg"),
        "[options.entry_points]\nconsole_scripts =\n    tool = pkg.cli:main\n",
    );
    write(&tmp.path().join("pkg/__init__.py"), "");
    write(&tmp.path().join("pkg/cli.py"), "def main():\n    return 0\n");
    let files = vec![
        tmp.path().join("pkg/__init__.py"),
        tmp.path().join("pkg/cli.py"),
    ];
    let (viols, _) = py_report(&files, &[], tmp.path());
    assert!(
        !orphan_names(&viols).iter().any(|n| n.ends_with("cli")),
        "setup.cfg console_scripts must not be orphan: {viols:#?}"
    );
}

#[test]
fn rust_cargo_bin_is_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"tool\"\npath = \"src/cli.rs\"\n",
    );
    write(&tmp.path().join("src/cli.rs"), "fn main() {}\n");
    let viols = rs_report(&[tmp.path().join("src/cli.rs")], &[], tmp.path());
    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "cargo bin cli.rs must not be orphan: {viols:#?}"
    );
}

#[test]
fn rust_cargo_example_is_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lib]\npath = \"src/lib.rs\"\n[[example]]\nname = \"demo\"\npath = \"examples/demo.rs\"\n",
    );
    write(&tmp.path().join("src/lib.rs"), "pub fn f() {}\n");
    write(&tmp.path().join("examples/demo.rs"), "fn main() {}\n");
    let files = vec![
        tmp.path().join("src/lib.rs"),
        tmp.path().join("examples/demo.rs"),
    ];
    let viols = rs_report(&files, &[], tmp.path());
    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.file.ends_with("demo.rs")),
        "cargo example must not be orphan: {viols:#?}"
    );
}

#[test]
fn orphan_allowed_exempts_plugin_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let plugin = tmp.path().join("src/plugins/hook.py");
    write(&plugin, "def run():\n    return 1\n");
    let (with_allow, _) = py_report(std::slice::from_ref(&plugin), &["src/plugins".into()], tmp.path());
    let (no_allow, _) = py_report(std::slice::from_ref(&plugin), &[], tmp.path());
    assert!(
        !with_allow.iter().any(|v| v.metric == "orphan_module"),
        "allowlisted plugin must not be orphan: {with_allow:#?}"
    );
    assert!(
        no_allow.iter().any(|v| v.metric == "orphan_module"),
        "plugin without allowlist must be orphan: {no_allow:#?}"
    );
}

#[test]
fn rust_path_attr_clears_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join("src/lib.rs"),
        "#[path = \"renamed.rs\"]\nmod foo;\n",
    );
    write(&tmp.path().join("src/renamed.rs"), "pub fn f() {}\n");
    let files = vec![
        tmp.path().join("src/lib.rs"),
        tmp.path().join("src/renamed.rs"),
    ];
    let viols = rs_report(&files, &[], tmp.path());
    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.file.ends_with("renamed.rs")),
        "#[path] target must not be orphan: {viols:#?}"
    );
}

#[test]
fn analyze_graph_false_emits_no_orphan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let utils = tmp.path().join("utils.py");
    write(&utils, "def f():\n    return 1\n");
    let parsed = parse_py(&utils);
    let roles = build_source_role_index(std::slice::from_ref(&parsed), &[], &[utils], &[]).unwrap();
    let ctx = build_python_context_graph(&[&parsed], &roles);
    let prod = ctx.production_view();
    let viols = analyze_graph(&prod, &Config::python_defaults(), false);
    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "analyze_graph(..., false) must not emit orphan_module: {viols:#?}"
    );
}

#[test]
fn orphan_scanners_do_not_mention_runtime_coverage() {
    let src = include_str!("orphan.rs");
    for needle in [".kiss", "profraw", "coverage"] {
        assert!(
            !src.contains(needle),
            "orphan.rs must not mention {needle}"
        );
    }
}
