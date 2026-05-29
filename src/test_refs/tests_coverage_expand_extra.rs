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

/// principles.md: partitions must not be global basename allowlists tuned to benchmark trees.
#[test]
fn calibration_path_partitions_are_not_global_basename_allowlists() {
    use crate::test_refs::detection::is_in_test_directory;
    assert!(
        !is_py_contrib_base_void_partition(std::path::Path::new(
            "acme/widgets/base/model.py"
        )),
        "unrelated layout: path segment \"base\" alone must not imply void partition"
    );
    assert!(
        !is_py_inflator_calibration_path(std::path::Path::new("lib/ops/deploy.py")),
        "unrelated layout: path segment \"ops\" alone must not imply inflator partition"
    );
    assert!(
        !is_py_inflator_calibration_path(std::path::Path::new(
            "reports/analysis/summary.py"
        )),
        "unrelated layout: path segment \"analysis\" alone must not imply inflator partition"
    );
    assert!(
        !is_in_test_directory(std::path::Path::new("ropetest/runtime_adapter.py")),
        "production module under a benchmark-shaped dirname must not count as test tree"
    );
    assert!(
        is_in_test_directory(std::path::Path::new("widget_tests/conftest_helpers.py")),
        "test trees named *_tests/ must not require a hardcoded vendor dirname"
    );
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
    assert!(!is_py_contrib_refactor_void_force_uncovered(std::path::Path::new(
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
    let test = dir.path().join("tests").join("test_versioning.py");
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
