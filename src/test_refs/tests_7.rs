use super::calibration_analysis::{
    build_calibration_coverage_refs, filter_unreferenced_definitions, UnreferencedFilterCtx,
};
use super::*;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[test]
fn test_build_calibration_coverage_refs_expands_witnesses() {
    use crate::parsing::{create_parser, parse_file};
    let mut parser = create_parser().expect("parser");
    let src = "def f():\n    g()\ndef g():\n    pass\n";
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").expect("tmp");
    std::io::Write::write_all(&mut tmp, src.as_bytes()).expect("write");
    let parsed = parse_file(&mut parser, tmp.path()).expect("parse");
    let refs_vec = [&parsed];
    let defs = vec![CodeDefinition {
        name: "f".into(),
        kind: CodeUnitKind::Function,
        file: parsed.path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    }];
    let witnesses = HashSet::from(["f".to_string()]);
    let (_strict, expanded, _counts) =
        build_calibration_coverage_refs(&refs_vec, &defs, &witnesses);
    assert!(expanded.contains("g"));
}

#[test]
fn test_module_definition_counts_from_graph() {
    use crate::graph::DependencyGraph;
    let path = PathBuf::from("/proj/mod.py");
    let def = CodeDefinition {
        name: "f".into(),
        kind: CodeUnitKind::Function,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(path, "mod".into());
    let orphan = CodeDefinition {
        name: "g".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("/proj/other.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let counts = coverage::module_definition_counts(&[def, orphan], &graph);
    assert_eq!(counts.get("mod"), Some(&1));
    assert_eq!(counts.len(), 1);
    assert!(coverage::module_definition_counts(&[], &graph).is_empty());
}

#[test]
fn test_defs_per_file_counts_and_filter_unreferenced_definitions() {
    let path = PathBuf::from("a.py");
    let def = CodeDefinition {
        name: "f".into(),
        kind: CodeUnitKind::Function,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let (_strict, _expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &HashSet::new());
    assert_eq!(counts.get(&path).copied(), Some(1));
    let ctx = UnreferencedFilterCtx {
        calibration: false,
        defs_per_file: &counts,
        test_witness_refs: &HashSet::new(),
        calibration_strict_refs: &HashSet::new(),
        calibration_expanded_refs: &HashSet::new(),
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &HashMap::new(),
        module_suffixes: &HashMap::new(),
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert_eq!(unref.len(), 1);
}

#[test]
fn test_void_partition_always_unreferenced_in_calibration() {
    let path = PathBuf::from("rope/contrib/findit.py");
    let def = CodeDefinition {
        name: "findit".into(),
        kind: CodeUnitKind::Function,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["findit".to_string()]);
    let (_strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &witnesses,
        calibration_expanded_refs: &expanded,
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &HashMap::new(),
        module_suffixes: &HashMap::new(),
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert_eq!(unref.len(), 1);
}

#[test]
fn test_base_tree_force_uncovered_in_calibration() {
    let path = PathBuf::from("rope/base/exceptions.py");
    let def = CodeDefinition {
        name: "exceptions".into(),
        kind: CodeUnitKind::Function,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["exceptions".to_string()]);
    let (_strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &witnesses,
        calibration_expanded_refs: &expanded,
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &HashMap::new(),
        module_suffixes: &HashMap::new(),
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert_eq!(unref.len(), 1);
}

#[test]
fn test_is_definition_covered_for_calibration_stem_match() {
    let def = CodeDefinition {
        name: "widget".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("widget.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let mut name_files = HashMap::new();
    name_files.insert("widget".to_string(), HashSet::from([PathBuf::from("widget.py")]));
    let refs = HashSet::from(["widget".to_string()]);
    assert!(coverage::is_definition_covered_for_calibration(
        &def,
        &name_files,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &refs,
    ));
    name_files.insert(
        "widget".to_string(),
        HashSet::from([PathBuf::from("widget.py"), PathBuf::from("other.py")]),
    );
    assert!(!coverage::is_definition_covered_for_calibration(
        &def,
        &name_files,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &refs,
    ));
}

#[test]
fn test_build_calibration_coverage_refs_counts_defs_per_file() {
    use super::calibration_analysis::{build_calibration_coverage_refs, defs_per_file_counts};
    use crate::parsing::{create_parser, parse_file};
    let mut lib = NamedTempFile::with_suffix(".py").unwrap();
    std::io::Write::write_all(
        &mut lib,
        b"def a():\n    pass\ndef b():\n    pass\n",
    )
    .unwrap();
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    std::io::Write::write_all(&mut testf, b"def test_x():\n    a()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let lib_p = parse_file(&mut parser, lib.path()).expect("parse");
    let test_p = parse_file(&mut parser, testf.path()).expect("parse");
    let parsed = [&lib_p, &test_p];
    let file = lib.path().to_path_buf();
    let definitions = vec![
        CodeDefinition {
            name: "a".into(),
            kind: CodeUnitKind::Function,
            file: file.clone(),
            line: 1,
            end_line: 2,
            containing_class: None,
        },
        CodeDefinition {
            name: "b".into(),
            kind: CodeUnitKind::Function,
            file: file.clone(),
            line: 3,
            end_line: 4,
            containing_class: None,
        },
    ];
    let witnesses = HashSet::from(["a".to_string()]);
    let (_, _, counts) = build_calibration_coverage_refs(&parsed, &definitions, &witnesses);
    assert_eq!(counts.get(&file).copied(), Some(2));
    assert_eq!(defs_per_file_counts(&definitions).get(&file).copied(), Some(2));
}
