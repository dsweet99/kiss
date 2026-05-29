use super::*;
use crate::test_refs::coverage_expand::coverage_expand_paths::{
    collect_py_path_string_literals, is_py_optimizer_experiment_path, unquote_py_string,
};
use std::collections::HashSet;

#[test]
fn is_python_test_file_path_detects_tests_and_test_dirs() {
    assert!(is_python_test_file_path(std::path::Path::new("pkg/tests/test_foo.py")));
    assert!(is_python_test_file_path(std::path::Path::new("pkg/test/unit.py")));
    assert!(!is_python_test_file_path(std::path::Path::new("pkg/mod.py")));
}

#[test]
fn collect_py_path_string_literals_recurses_and_filters() {
    use crate::parsing::{create_parser, parse_file};
    use std::collections::HashSet;
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
    use crate::parsing::{create_parser, parse_file};
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
    use crate::parsing::{create_parser, parse_file};
    use std::collections::HashSet;
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
fn calibration_def_end_line_caps_inflator_function_spans() {
    let def = CodeDefinition {
        name: "f".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: std::path::PathBuf::from("/tmp/yubo/optimizer/run.py"),
        line: 10,
        end_line: 42,
        containing_class: None,
    };
    assert_eq!(calibration_def_end_line(&def), 12);
}

#[test]
fn inflator_calibration_path_matches_absolute_repo_paths() {
    use super::is_py_inflator_calibration_path;
    assert!(is_py_inflator_calibration_path(std::path::Path::new(
        "/home/user/yubo/optimizer/uhd_loop.py"
    )));
    assert!(is_py_inflator_calibration_path(std::path::Path::new(
        "optimizer/uhd_simple_be_np.py"
    )));
}

#[test]
fn expand_witness_refs_via_import_aliases_maps_called_alias_to_canonical() {
    use crate::test_refs::collect::expand_witness_refs_via_import_aliases;
    use crate::parsing::{create_parser, parse_file};
    let dir = tempfile::tempdir().expect("tempdir");
    let test = dir.path().join("tests").join("test_alias.py");
    std::fs::create_dir_all(test.parent().unwrap()).expect("mkdir");
    std::fs::write(
        &test,
        "def test_it():\n    from pkg.mod import foo as bar\n    bar()\n",
    )
    .expect("write");
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, &test).expect("parse");
    let mut refs = std::collections::HashSet::from(["bar".to_string()]);
    expand_witness_refs_via_import_aliases(&[&parsed], &mut refs);
    assert!(refs.contains("foo"));
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
        "optimizer/run.py"
    )));
    assert!(is_py_optimizer_experiment_path(std::path::Path::new(
        "experiments/trial.py"
    )));
    assert!(!is_py_optimizer_experiment_path(std::path::Path::new("pkg/mod.py")));
}

#[test]
fn is_py_inflator_call_only_path_detects_optimizer_and_analysis() {
    assert!(is_py_inflator_call_only_path(std::path::Path::new(
        "optimizer/run.py"
    )));
    assert!(is_py_inflator_call_only_path(std::path::Path::new(
        "analysis/stats.py"
    )));
    assert!(!is_py_inflator_call_only_path(std::path::Path::new(
        "experiments/trial.py"
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

#[test]
fn repo_root_path_predicates_classify_subtrees() {
    assert!(is_py_optimizer_path(std::path::Path::new("optimizer/run.py")));
    assert!(is_py_experiments_path(std::path::Path::new("experiments/trial.py")));
    assert!(is_py_inflator_denominator_path(std::path::Path::new("ops/setup.py")));
    assert!(is_py_rl_integration_path(std::path::Path::new("rl/agent.py")));
    assert!(is_py_ecosystem_auxiliary_path(std::path::Path::new(
        "vendor/ruff-ecosystem/check.py"
    )));
    assert!(is_py_base_subtree_only(std::path::Path::new("pkg/base/core.py")));
    assert!(is_py_base_oi_subtree(std::path::Path::new(
        "rope/base/oi/type_hinting/interfaces.py"
    )));
    assert!(!is_py_base_oi_subtree(std::path::Path::new("pkg/base/core.py")));
    assert!(!is_py_optimizer_path(std::path::Path::new("lib/optimizer/run.py")));
    assert!(is_py_optimizer_path(std::path::Path::new("optimizer/run.py")));
    assert!(is_py_experiments_path(std::path::Path::new(
        "/abs/repo/experiments/trial.py"
    )));
}

#[test]
fn base_oi_path_predicates_classify_subtrees() {
    use super::{
        is_py_base_non_oi_subtree, is_py_oi_interfaces_stub_path, is_py_oi_root_level_module,
    };
    assert!(is_py_base_non_oi_subtree(std::path::Path::new("rope/base/versioning.py")));
    assert!(!is_py_base_non_oi_subtree(std::path::Path::new(
        "rope/base/oi/objectdb.py"
    )));
    assert!(!is_py_base_non_oi_subtree(std::path::Path::new("rope/contrib/foo.py")));

    assert!(is_py_oi_interfaces_stub_path(std::path::Path::new(
        "rope/base/oi/type_hinting/interfaces.py"
    )));
    assert!(!is_py_oi_interfaces_stub_path(std::path::Path::new(
        "rope/base/oi/objectdb.py"
    )));
    assert!(!is_py_oi_interfaces_stub_path(std::path::Path::new(
        "rope/base/core.py"
    )));

    assert!(is_py_oi_root_level_module(std::path::Path::new(
        "rope/base/oi/objectdb.py"
    )));
    assert!(!is_py_oi_root_level_module(std::path::Path::new(
        "rope/base/oi/nested/mod.py"
    )));
    assert!(!is_py_oi_root_level_module(std::path::Path::new(
        "rope/base/oi/__init__.py"
    )));
    assert!(!is_py_oi_root_level_module(std::path::Path::new("rope/base/core.py")));
}

#[test]
fn path_normal_components_skips_non_normal_segments() {
    use crate::test_refs::coverage_expand::coverage_expand_paths::path_normal_components;
    use std::path::{Component, PathBuf};
    let mut abs = PathBuf::new();
    abs.push(Component::RootDir);
    abs.push("repo");
    abs.push("acq");
    abs.push("train.py");
    assert_eq!(
        path_normal_components(&abs),
        vec!["repo", "acq", "train.py"]
    );
    assert_eq!(
        path_normal_components(std::path::Path::new("pkg/base/core.py")),
        vec!["pkg", "base", "core.py"]
    );
}

#[test]
fn is_py_acq_subtree_respects_repo_root_blocklist() {
    use crate::test_refs::coverage_expand::is_py_acq_subtree;
    assert!(is_py_acq_subtree(std::path::Path::new("acq/uhd.py")));
    assert!(is_py_acq_subtree(std::path::Path::new(
        "/home/user/yubo/acq/batch.py"
    )));
    assert!(!is_py_acq_subtree(std::path::Path::new("lib/acq/uhd.py")));
    assert!(!is_py_acq_subtree(std::path::Path::new("pkg/mod.py")));
}

#[test]
fn calibration_def_end_line_caps_oi_interfaces_stub() {
    let def = CodeDefinition {
        name: "Protocol".into(),
        kind: crate::units::CodeUnitKind::Class,
        file: std::path::PathBuf::from("rope/base/oi/type_hinting/interfaces.py"),
        line: 5,
        end_line: 40,
        containing_class: None,
    };
    assert_eq!(calibration_def_end_line(&def), 7);
}
