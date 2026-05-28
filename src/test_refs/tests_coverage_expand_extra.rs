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
fn inflator_calibration_path_matches_yubo_subtrees() {
    use super::is_py_inflator_calibration_path;
    assert!(is_py_inflator_calibration_path(std::path::Path::new(
        "optimizer/uhd_simple_be_np.py"
    )));
    assert!(is_py_inflator_calibration_path(std::path::Path::new(
        "analysis/plotting_2_pareto.py"
    )));
    assert!(is_py_inflator_calibration_path(std::path::Path::new(
        "ops/uhd_setup.py"
    )));
    assert!(!is_py_inflator_calibration_path(std::path::Path::new(
        "common/seed_all.py"
    )));
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
fn raises_and_patch_literals_collected() {
    let mut src = NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        r#"import unittest
from unittest.mock import patch
class T(unittest.TestCase):
    def test_x(self):
        with self.assertRaises(pkg.mod.MyError):
            pass
    def test_y(self):
        with patch("pkg.mod.target", return_value=1):
            pass
    def test_z(self):
        with patch('pkg.mod.other'):
            pass
    def test_w(self):
        with self.assertRaises(LocalError):
            pass
    def test_bad(self):
        with pytest.raises(123):
            pass
"#,
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let root = parsed.tree.root_node();
    let mut dotted = HashSet::new();
    collect_py_raises_dotted_paths(root, &parsed.source, &mut dotted);
    collect_py_dynamic_dispatch_literals(root, &parsed.source, &mut dotted);
    assert!(dotted.iter().any(|d| d == "pkg.mod.MyError"));
    assert!(dotted.iter().any(|d| d.contains("target")));
    assert!(dotted.iter().any(|d| d == "LocalError"));
}

#[test]
fn void_dispatch_attests_patch_target_in_void_partition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("rope/base/versioning.py");
    std::fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
    std::fs::write(&target, "def f():\n    pass\n").expect("write");
    let test = dir.path().join("ropetest/versioningtest.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(
        &test,
        "import unittest\nfrom unittest.mock import patch\nclass T(unittest.TestCase):\n    def test_v(self):\n        with patch('rope.base.versioning._get_file_content'):\n            pass\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed_test = parse_file(&mut parser, &test).expect("parse test");
    let parsed_target = parse_file(&mut parser, &target).expect("parse target");
    let mut suffixes = std::collections::HashMap::new();
    suffixes.insert(target.clone(), "rope.base.versioning".to_string());
    let attested = build_py_void_dispatch_attestation(&[&parsed_test, &parsed_target], &suffixes);
    assert!(attested.contains_key(&target));
    assert!(attested[&target].contains("_get_file_content"));
}

#[test]
fn is_in_test_directory_includes_ropetest() {
    use crate::test_refs::detection::is_in_test_directory;
    assert!(is_in_test_directory(std::path::Path::new("ropetest/versioningtest.py")));
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

#[test]
fn unquote_py_string_strips_quotes() {
    assert_eq!(unquote_py_string("\"a.b\""), "a.b");
    assert_eq!(unquote_py_string("'a.b'"), "a.b");
    assert_eq!(unquote_py_string("  \"x\"  "), "x");
}

#[test]
fn calibration_def_end_line_returns_def_end_line() {
    let def = CodeDefinition {
        name: "f".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: std::path::PathBuf::from("mod.py"),
        line: 1,
        end_line: 42,
        containing_class: None,
    };
    assert_eq!(calibration_def_end_line(&def), 42);
}

#[test]
fn is_py_optimizer_experiment_path_detects_subtrees() {
    assert!(is_py_optimizer_experiment_path(std::path::Path::new(
        "pkg/optimizer/run.py"
    )));
    assert!(is_py_optimizer_experiment_path(std::path::Path::new(
        "pkg/experiments/trial.py"
    )));
    assert!(!is_py_optimizer_experiment_path(std::path::Path::new("pkg/mod.py")));
}

#[test]
fn is_py_inflator_call_only_path_detects_optimizer_and_analysis() {
    assert!(is_py_inflator_call_only_path(std::path::Path::new(
        "pkg/optimizer/run.py"
    )));
    assert!(is_py_inflator_call_only_path(std::path::Path::new(
        "pkg/analysis/stats.py"
    )));
    assert!(!is_py_inflator_call_only_path(std::path::Path::new(
        "pkg/experiments/trial.py"
    )));
}

#[test]
fn void_files_for_dotted_path_resolves_contrib_modules() {
    let mut suffixes = std::collections::HashMap::new();
    let path = std::path::PathBuf::from("rope/contrib/findit.py");
    suffixes.insert(path.clone(), "rope.contrib.findit".to_string());
    let files = void_files_for_dotted_path("rope.contrib.findit.findit", &suffixes);
    assert!(files.contains(&path));
}
