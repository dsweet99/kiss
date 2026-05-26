use super::*;
use crate::parsing::{create_parser, parse_file};
use std::collections::HashSet;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn expand_py_usage_refs_one_hop_from_covered_fn() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "def helper():\n    pass\n\ndef caller():\n    helper()\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let refs = [&parsed];
    let mut usage = HashSet::from(["caller".to_string()]);
    expand_py_usage_refs_fixpoint(&refs, &mut usage);
    assert!(usage.contains("helper"));
}

#[test]
fn expand_py_usage_refs_one_hop_from_covered_class() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "class Foo:\n    def bar(self):\n        baz()\n\ndef baz():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let refs = [&parsed];
    let mut usage = HashSet::from(["Foo".to_string()]);
    expand_py_usage_refs_fixpoint(&refs, &mut usage);
    assert!(usage.contains("baz"));
}

#[test]
fn merge_py_body_refs_direct() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "def fn():\n    seen()\n    novel()\ndef seen():\n    pass\ndef novel():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let root = parsed.tree.root_node();
    let fn_node = root.child(0).unwrap();
    let refs = HashSet::from(["seen".to_string()]);
    let mut added = HashSet::new();
    merge_py_body_refs(fn_node, &parsed.source, &refs, &mut added);
    assert!(!added.contains("seen"));
    assert!(added.contains("novel"));
}

#[test]
fn one_hop_py_refs_skips_test_files() {
    let mut prod = NamedTempFile::with_suffix(".py").unwrap();
    write!(prod, "def caller():\n    helper()\ndef helper():\n    pass\n").unwrap();
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(testf, "def test_x():\n    helper()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let prod_parsed = parse_file(&mut parser, prod.path()).expect("parse");
    let test_parsed = parse_file(&mut parser, testf.path()).expect("parse");
    let files = [&prod_parsed, &test_parsed];
    let usage = HashSet::from(["caller".to_string()]);
    let added = one_hop_py_refs(&files, &usage);
    assert!(added.contains("helper"));
}

#[test]
fn collect_one_hop_skips_uncalled_functions() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "def uncalled():\n    ghost()\ndef caller():\n    leaf()\ndef leaf():\n    pass\ndef ghost():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let files = [&parsed];
    let usage = HashSet::from(["caller".to_string()]);
    let added = one_hop_py_refs(&files, &usage);
    assert!(added.contains("leaf"));
    assert!(!added.contains("ghost"));
}

#[test]
fn collect_one_hop_expands_class_method_when_class_in_refs() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "class Foo:\n    def bar(self):\n        baz()\ndef baz():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let files = [&parsed];
    let usage = HashSet::from(["Foo".to_string()]);
    let added = one_hop_py_refs(&files, &usage);
    assert!(added.contains("baz"));
}

#[test]
fn collect_one_hop_walks_module_level_after_class() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "class Foo:\n    def bar(self):\n        baz()\ndef baz():\n    pass\ndef unrelated():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let files = [&parsed];
    let usage = HashSet::from(["Foo".to_string()]);
    let added = one_hop_py_refs(&files, &usage);
    assert!(added.contains("baz"));
}

#[test]
fn resolve_relative_module_suffix_one_and_two_dots() {
    assert_eq!(
        resolve_relative_module_suffix("rich.console", ".terminal_theme").as_deref(),
        Some("rich.terminal_theme")
    );
    assert_eq!(
        resolve_relative_module_suffix("pkg.sub.mod", "..sibling").as_deref(),
        Some("pkg.sibling")
    );
}

#[test]
fn expand_py_import_sibling_refs_nested_in_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = dir.path().join("pkg");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    let path = pkg.join("mod.py");
    std::fs::write(
        &path,
        "def outer():\n    pass\nfrom .helper import used, unused\n",
    )
    .expect("write");
    std::fs::write(pkg.join("helper.py"), "used = 1\nunused = 2\n").expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &path).expect("parse");
    let files = [&parsed];
    let mut usage = HashSet::from(["used".to_string()]);
    expand_py_import_sibling_refs(&files, &mut usage);
    assert!(usage.contains("unused"));
}

#[test]
fn merge_import_sibling_names_skips_empty_relative_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mod.py");
    std::fs::write(&path, "from . import sibling\n").expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &path).expect("parse");
    let root = parsed.tree.root_node();
    let import_node = root
        .child(0)
        .filter(|n| n.kind() == "import_from_statement")
        .expect("import");
    let mut usage = HashSet::from(["sibling".to_string()]);
    merge_import_sibling_names(import_node, &parsed.source, "pkg.mod", &mut usage);
    assert!(usage.contains("sibling"));
}

#[test]
fn import_names_from_statement_collects_aliased() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    writeln!(src, "from mod import foo as bar").unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let import_node = parsed.tree.root_node().child(0).unwrap();
    let names = import_names_from_statement(import_node, &parsed.source);
    assert!(names.contains("foo") || names.contains("bar"));
}

#[test]
fn collect_import_siblings_walks_module_body() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(src, "class Wrapper:\n    pass\nfrom pkg import one, two\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let mut usage = HashSet::from(["one".to_string()]);
    collect_import_siblings(parsed.tree.root_node(), &parsed.source, "mod", &mut usage);
    assert!(usage.contains("two"));
}

#[test]
fn expand_py_import_sibling_refs_noop_when_no_witness() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        src,
        "from terminal_theme import DEFAULT_TERMINAL_THEME, TerminalTheme"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let files = [&parsed];
    let mut usage = HashSet::from(["Unrelated".to_string()]);
    expand_py_import_sibling_refs(&files, &mut usage);
    assert!(!usage.contains("TerminalTheme"));
}

#[test]
fn expand_py_import_sibling_refs_absolute_import() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        src,
        "from terminal_theme import DEFAULT_TERMINAL_THEME, TerminalTheme"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let files = [&parsed];
    let mut usage = HashSet::from(["DEFAULT_TERMINAL_THEME".to_string()]);
    expand_py_import_sibling_refs(&files, &mut usage);
    assert!(usage.contains("TerminalTheme"));
}

#[test]
fn expand_py_import_sibling_refs_adds_co_imported_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = dir.path().join("rich");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    let path = pkg.join("console.py");
    std::fs::write(
        &path,
        "from .terminal_theme import DEFAULT_TERMINAL_THEME, TerminalTheme\n\nclass Console:\n    def render(self):\n        return DEFAULT_TERMINAL_THEME\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &path).expect("parse");
    let files = [&parsed];
    let mut usage = HashSet::from(["DEFAULT_TERMINAL_THEME".to_string()]);
    expand_py_import_sibling_refs(&files, &mut usage);
    assert!(usage.contains("TerminalTheme"));
}

#[test]
fn collect_one_hop_from_node_direct_async_and_plain() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "async def caller():\n    await other()\nasync def other():\n    pass\ndef plain():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let root = parsed.tree.root_node();
    let refs = HashSet::from(["caller".to_string()]);
    let mut added = HashSet::new();
    collect_one_hop_from_node(root, &parsed.source, &refs, &mut added, None);
    assert!(added.contains("other"));
}
