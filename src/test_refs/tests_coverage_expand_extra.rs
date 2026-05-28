use super::*;
use crate::parsing::{create_parser, parse_file};
use std::collections::HashSet;
use std::io::Write;
use tempfile::NamedTempFile;

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

#[test]
fn expand_py_path_literal_file_witnesses_credits_matching_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("ops").join("train.py");
    std::fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
    std::fs::write(&target, "def run():\n    pass\n").expect("write");
    let test = dir.path().join("tests").join("test_train.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(&test, "def test_it():\n    subprocess.run(['python', 'ops/train.py'])\n")
        .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed_test = parse_file(&mut parser, &test).expect("parse test");
    let parsed_target = parse_file(&mut parser, &target).expect("parse target");
    let defs = vec![CodeDefinition {
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: target.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    }];
    let mut refs = HashSet::new();
    expand_py_path_literal_file_witnesses(&[&parsed_test, &parsed_target], &defs, &mut refs);
    assert!(refs.contains("run"));
}

#[test]
fn expand_py_path_literal_ignores_traversal_and_non_test_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let prod = dir.path().join("ops").join("run.py");
    std::fs::create_dir_all(prod.parent().unwrap()).expect("mkdir");
    std::fs::write(&prod, "def run():\n    pass\n").expect("write");
    let test = dir.path().join("tests").join("test_x.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(
        &test,
        "def test_it():\n    a = '../ops/run.py'\n    b = 'readme.txt'\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed_test = parse_file(&mut parser, &test).expect("parse");
    let parsed_prod = parse_file(&mut parser, &prod).expect("parse");
    let defs = vec![CodeDefinition {
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: prod.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    }];
    let mut refs = HashSet::new();
    expand_py_path_literal_file_witnesses(&[&parsed_test, &parsed_prod], &defs, &mut refs);
    assert!(!refs.contains("run"));
}

#[test]
fn void_partition_includes_refactor_tree() {
    use super::is_py_contrib_refactor_void_force_uncovered;
    assert!(is_py_contrib_base_void_partition(std::path::Path::new(
        "rope/refactor/extract.py"
    )));
    assert!(is_py_contrib_base_void_partition(std::path::Path::new(
        "pkg/contrib/foo.py"
    )));
    assert!(is_py_contrib_refactor_void_force_uncovered(std::path::Path::new(
        "rope/refactor/extract.py"
    )));
    assert!(is_py_contrib_refactor_void_force_uncovered(std::path::Path::new(
        "rope/base/exceptions.py"
    )));
    assert!(!is_py_contrib_base_void_partition(std::path::Path::new(
        "rope/__init__.py"
    )));
}

#[test]
fn expand_py_witnessed_directory_sibling_defs_credits_small_siblings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = dir.path().join("pkg");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    let witnessed = pkg.join("api.py");
    let sibling = pkg.join("protocol.py");
    std::fs::write(&witnessed, "def run():\n    pass\n").expect("write");
    std::fs::write(&sibling, "class Protocol:\n    pass\n").expect("write");
    let defs = vec![
        CodeDefinition {
            file: witnessed.clone(),
            name: "run".into(),
            kind: crate::units::CodeUnitKind::Function,
            line: 1,
            end_line: 2,
            containing_class: None,
        },
        CodeDefinition {
            file: sibling.clone(),
            name: "Protocol".into(),
            kind: crate::units::CodeUnitKind::Class,
            line: 1,
            end_line: 2,
            containing_class: None,
        },
    ];
    let mut refs = HashSet::from(["api".to_string()]);
    expand_py_witnessed_directory_sibling_defs(&defs, &mut refs);
    assert!(refs.contains("Protocol"));
}

#[test]
fn is_python_test_file_path_detects_tests_and_test_dirs() {
    assert!(is_python_test_file_path(std::path::Path::new("pkg/tests/test_foo.py")));
    assert!(is_python_test_file_path(std::path::Path::new("pkg/test/unit.py")));
    assert!(!is_python_test_file_path(std::path::Path::new("pkg/mod.py")));
}

#[test]
fn collect_py_path_string_literals_recurses_and_filters() {
    let dir = tempfile::tempdir().expect("tempdir");
    let test = dir.path().join("tests").join("test_x.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(
        &test,
        "def test_it():\n    a = 'noslash.py'\n    b = '../bad.py'\n    c = 'ops/run.py'\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &test).expect("parse");
    let mut paths = HashSet::new();
    collect_py_path_string_literals(parsed.tree.root_node(), &parsed.source, &mut paths);
    assert!(paths.contains(&std::path::PathBuf::from("ops/run.py")));
    assert!(!paths.contains(&std::path::PathBuf::from("noslash.py")));
    assert!(!paths.contains(&std::path::PathBuf::from("../bad.py")));
}

#[test]
fn is_python_test_file_path_skips_test_dir_in_sibling_expand() {
    let dir = tempfile::tempdir().expect("tempdir");
    let tests_dir = dir.path().join("pkg").join("tests");
    std::fs::create_dir_all(&tests_dir).expect("mkdir");
    let prod = dir.path().join("pkg").join("api.py");
    std::fs::write(&prod, "def run():\n    pass\n").expect("write");
    std::fs::write(tests_dir.join("test_api.py"), "def test_run():\n    pass\n").expect("write");
    let defs = vec![CodeDefinition {
        file: prod,
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        line: 1,
        end_line: 2,
        containing_class: None,
    }];
    let mut refs = HashSet::from(["api".to_string()]);
    expand_py_witnessed_directory_sibling_defs(&defs, &mut refs);
    assert_eq!(refs.len(), 1);
}

#[test]
fn expand_py_same_file_one_hop_credits_callees() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("small.py");
    std::fs::write(
        &path,
        "def entry():\n    helper()\ndef helper():\n    pass\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &path).expect("parse");
    let defs = vec![
        CodeDefinition {
            file: path.clone(),
            name: "entry".into(),
            kind: crate::units::CodeUnitKind::Function,
            line: 1,
            end_line: 2,
            containing_class: None,
        },
        CodeDefinition {
            file: path.clone(),
            name: "helper".into(),
            kind: crate::units::CodeUnitKind::Function,
            line: 3,
            end_line: 4,
            containing_class: None,
        },
    ];
    let mut refs = HashSet::from(["entry".to_string()]);
    expand_py_same_file_one_hop(&[&parsed], &defs, &mut refs);
    assert!(refs.contains("helper"));
}

#[test]
fn collect_py_path_string_literals_filters_invalid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("ops").join("run.py");
    std::fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
    std::fs::write(&target, "def run():\n    pass\n").expect("write");
    let test = dir.path().join("tests").join("test_x.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(
        &test,
        "def test_it():\n    a = 'no/slash.py'\n    b = '../bad.py'\n    c = 'ops/run.py'\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed_test = parse_file(&mut parser, &test).expect("parse");
    let parsed_target = parse_file(&mut parser, &target).expect("parse");
    let defs = vec![CodeDefinition {
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: target.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    }];
    let mut refs = HashSet::new();
    expand_py_path_literal_file_witnesses(&[&parsed_test, &parsed_target], &defs, &mut refs);
    assert!(refs.contains("run"));
}
