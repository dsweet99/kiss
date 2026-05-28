use super::calibration_analysis::{
    build_calibration_coverage_refs, filter_unreferenced_definitions, UnreferencedFilterCtx,
};
use super::*;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::io::Write;
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
    let no_module = CodeDefinition {
        name: "orphan".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("/unmapped/other.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let counts = coverage::module_definition_counts(&[def, orphan, no_module], &graph);
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
        void_dispatch_attestation: &HashMap::new(),
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
fn test_void_partition_unreferenced_without_dispatch_attestation() {
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
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &HashMap::new(),
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
fn test_void_partition_strict_witness_covers_with_dispatch_attestation() {
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
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let mut attested = HashMap::new();
    attested.insert(path.clone(), HashSet::from(["findit".to_string()]));
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &attested,
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &HashMap::new(),
        module_suffixes: &HashMap::new(),
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert!(unref.is_empty());
}

#[test]
fn test_base_tree_force_uncovered_without_dispatch_attestation() {
    let path = PathBuf::from("rope/base/exceptions.py");
    let def = CodeDefinition {
        name: "ResourceNotFoundError".into(),
        kind: CodeUnitKind::Class,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["ResourceNotFoundError".to_string()]);
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &HashMap::new(),
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
fn test_base_tree_strict_witness_can_cover_with_dispatch_attestation() {
    let path = PathBuf::from("rope/base/exceptions.py");
    let def = CodeDefinition {
        name: "ResourceNotFoundError".into(),
        kind: CodeUnitKind::Class,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["ResourceNotFoundError".to_string()]);
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let mut attestation = HashMap::new();
    attestation.insert(
        path.clone(),
        HashSet::from(["ResourceNotFoundError".to_string()]),
    );
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &attestation,
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &HashMap::new(),
        module_suffixes: &HashMap::new(),
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert!(unref.is_empty());
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

#[test]
fn test_is_py_package_init_import_witnessed() {
    let init = PathBuf::from("/proj/pkg/__init__.py");
    let def = CodeDefinition {
        name: "bootstrap".into(),
        kind: CodeUnitKind::Function,
        file: init.clone(),
        line: 1,
        end_line: 3,
        containing_class: None,
    };
    let mut bindings = HashMap::new();
    bindings.insert("pkg.sub".into(), HashSet::from(["Thing".into()]));
    let mut suffixes = HashMap::new();
    suffixes.insert(init.clone(), "proj.pkg.__init__".into());
    assert!(coverage::is_py_package_init_import_witnessed(
        &def, &bindings, &suffixes
    ));
    bindings.clear();
    bindings.insert("other.mod".into(), HashSet::new());
    assert!(!coverage::is_py_package_init_import_witnessed(
        &def, &bindings, &suffixes
    ));
    let mut bindings2 = HashMap::new();
    bindings2.insert("pkg".into(), HashSet::new());
    assert!(coverage::is_py_package_init_import_witnessed(
        &def, &bindings2, &HashMap::new()
    ));
}

#[test]
fn test_deprioritize_class_name_witness_in_non_gated_test() {
    use crate::parsing::{create_parser, parse_file};
    let mut prod = NamedTempFile::with_suffix("_windows.py").unwrap();
    write!(prod, "class Win:\n    def api(self):\n        pass\n").unwrap();
    let mut gated = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(
        gated,
        "import sys\nif sys.platform != 'win32':\n    pass\n"
    )
    .unwrap();
    let mut direct = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(direct, "def test_win():\n    Win()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let prod_p = parse_file(&mut parser, prod.path()).expect("parse");
    let gated_p = parse_file(&mut parser, gated.path()).expect("parse");
    let direct_p = parse_file(&mut parser, direct.path()).expect("parse");
    let parsed = [&prod_p, &gated_p, &direct_p];
    let analysis = analyze_test_refs_for_coverage_map(&parsed, None);
    let mut unreferenced = analysis.unreferenced.clone();
    let per_test = super::collect_parallel::collect_refs_parallel_for_coverage_map(&parsed).4;
    coverage::deprioritize_platform_gated_coverage(
        &analysis.definitions,
        &mut unreferenced,
        &per_test,
        &parsed,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
    assert!(
        !unreferenced.iter().any(|d| d.name == "api"),
        "class-name witness in non-gated test should keep method covered"
    );
}
