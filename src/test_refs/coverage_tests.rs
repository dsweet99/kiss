use super::coverage::{module_has_usage_witness, platform_direct_test_witness};
use super::{CodeDefinition, PerTestUsage};
use crate::graph::DependencyGraph;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[test]
fn platform_direct_test_witness_paths() {
    let gated: HashSet<&Path> = HashSet::from([Path::new("tests/gated_test.py")]);
    let def = CodeDefinition {
        name: "api".into(),
        kind: CodeUnitKind::Method,
        file: PathBuf::from("rich/_windows.py"),
        line: 1,
        end_line: 2,
        containing_class: Some("Win".into()),
    };
    let per_test: PerTestUsage = vec![
        (
            PathBuf::from("tests/gated_test.py"),
            vec![("test_gated".into(), HashSet::from(["api".into()]))],
        ),
        (
            PathBuf::from("tests/test_ok.py"),
            vec![("test_ok".into(), HashSet::from(["Win".into()]))],
        ),
    ];
    assert!(!platform_direct_test_witness(&def, &per_test, &gated));
    let per_test_direct: PerTestUsage = vec![(
        PathBuf::from("tests/test_ok.py"),
        vec![("test_ok".into(), HashSet::from(["api".into()]))],
    )];
    assert!(platform_direct_test_witness(&def, &per_test_direct, &gated));
}

#[test]
fn module_has_usage_witness_paths() {
    let mut graph = DependencyGraph::new();
    let path = PathBuf::from("/proj/helper.py");
    graph
        .path_to_module
        .insert(path.clone(), "helper".to_string());
    let definitions = vec![CodeDefinition {
        name: "helper_only".into(),
        kind: CodeUnitKind::Function,
        file: path,
        line: 1,
        end_line: 2,
        containing_class: None,
    }];
    let name_files = crate::test_refs::build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let empty: HashSet<String> = HashSet::new();
    assert!(!module_has_usage_witness(
        "helper",
        &definitions,
        &graph,
        &empty,
        &name_files
    ));
    let mut usage = HashSet::new();
    usage.insert("helper_only".into());
    assert!(module_has_usage_witness(
        "helper",
        &definitions,
        &graph,
        &usage,
        &name_files
    ));
}

type ImportCalibrationFixture = (
    CodeDefinition,
    HashMap<String, HashSet<String>>,
    HashMap<PathBuf, String>,
    HashSet<String>,
);

fn import_calibration_fixture() -> ImportCalibrationFixture {
    let file = PathBuf::from("/proj/pkg/mod.py");
    let def = CodeDefinition {
        name: "target".into(),
        kind: CodeUnitKind::Function,
        file: file.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let mut import_bindings = HashMap::new();
    import_bindings.insert("pkg.mod".into(), HashSet::from(["target".into()]));
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(file, "pkg.mod".into());
    let usage = HashSet::from(["target".into()]);
    (def, import_bindings, module_suffixes, usage)
}

#[test]
fn is_covered_by_import_calibration_paths() {
    use super::coverage::{is_covered_by_import, is_covered_by_import_for_calibration};
    let (def, import_bindings, module_suffixes, usage) = import_calibration_fixture();
    let mut name_files = HashMap::new();
    name_files.insert(
        "target".into(),
        HashSet::from([
            PathBuf::from("/proj/pkg/mod.py"),
            PathBuf::from("/proj/other.py"),
        ]),
    );
    assert!(is_covered_by_import(
        &def,
        &import_bindings,
        &module_suffixes,
        &usage
    ));
    assert!(is_covered_by_import_for_calibration(
        &def,
        &import_bindings,
        &module_suffixes,
        &usage,
        &name_files
    ));
    let mut other_def = def.clone();
    other_def.file = PathBuf::from("/proj/other.py");
    assert!(!is_covered_by_import_for_calibration(
        &other_def,
        &import_bindings,
        &module_suffixes,
        &usage,
        &name_files
    ));
    name_files.insert(
        "target".into(),
        HashSet::from([PathBuf::from("/proj/pkg/mod.py")]),
    );
    assert!(is_covered_by_import_for_calibration(
        &def,
        &import_bindings,
        &module_suffixes,
        &usage,
        &name_files
    ));
}

#[test]
fn base_module_import_witnessed_when_module_imported() {
    use super::coverage::is_py_base_module_import_witnessed;
    let def = CodeDefinition {
        name: "TaskHandle".into(),
        kind: CodeUnitKind::Class,
        file: PathBuf::from("rope/base/taskhandle.py"),
        line: 1,
        end_line: 10,
        containing_class: None,
    };
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(def.file.clone(), "rope.base.taskhandle".into());
    let mut import_bindings = HashMap::new();
    import_bindings.insert("rope.base.taskhandle".into(), HashSet::new());
    assert!(is_py_base_module_import_witnessed(
        &def,
        &import_bindings,
        &module_suffixes,
    ));
    import_bindings.clear();
    import_bindings.insert("rope.base".into(), HashSet::new());
    assert!(is_py_base_module_import_witnessed(
        &def,
        &import_bindings,
        &module_suffixes,
    ));
}

#[test]
fn import_matches_and_module_definition_counts_paths() {
    use super::coverage::{import_matches_definition, module_definition_counts};
    use super::TestRefAnalysis;
    let (def, import_bindings, module_suffixes, usage) = import_calibration_fixture();
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(PathBuf::from("/proj/pkg/mod.py"), "pkg.mod".into());
    let counts = module_definition_counts(std::slice::from_ref(&def), &graph);
    assert_eq!(counts.get("pkg.mod").copied(), Some(1));
    let orphan = CodeDefinition {
        name: "orphan".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("/proj/no_graph.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    assert!(module_definition_counts(&[orphan], &graph).is_empty());
    let mut ib_wrong = import_bindings.clone();
    ib_wrong.insert("pkg.mod".into(), HashSet::from(["other".into()]));
    assert!(!import_matches_definition(
        &def,
        &ib_wrong,
        &module_suffixes,
        &usage
    ));
    let analysis3 = TestRefAnalysis {
        definitions: vec![
            def.clone(),
            CodeDefinition {
                name: "second".into(),
                kind: CodeUnitKind::Function,
                file: PathBuf::from("/proj/pkg/mod.py"),
                line: 3,
                end_line: 4,
                containing_class: None,
            },
        ],
        test_references: HashSet::new(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
    assert_eq!(
        module_definition_counts(&analysis3.definitions, &graph)
            .get("pkg.mod")
            .copied(),
        Some(2)
    );
}

#[test]
fn module_is_contrib_base_void_paths() {
    use super::coverage::module_is_contrib_base_void;
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/normal.py"), "pkg.normal".into());
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/contrib/hook.py"), "pkg.contrib.hook".into());
    let normal = CodeDefinition {
        name: "run".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/normal.py"),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let contrib = CodeDefinition {
        name: "hook".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/contrib/hook.py"),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    assert!(!module_is_contrib_base_void("pkg.normal", std::slice::from_ref(&normal), &graph));
    assert!(module_is_contrib_base_void(
        "pkg.contrib.hook",
        std::slice::from_ref(&contrib),
        &graph
    ));
}

#[test]
fn is_pragma_no_cover_def_detects_same_line_and_previous() {
    use super::coverage::{deprioritize_pragma_no_cover_coverage, is_pragma_no_cover_def};
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;
    let mut src = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(
        src,
        "# pragma: no cover\ndef hidden():\n    pass\ndef visible():  # pragma: no cover\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, src.path()).expect("parse");
    let parsed_ref = [&parsed];
    let hidden = CodeDefinition {
        name: "hidden".into(),
        kind: CodeUnitKind::Function,
        file: parsed.path.clone(),
        line: 2,
        end_line: 3,
        containing_class: None,
    };
    let visible = CodeDefinition {
        name: "visible".into(),
        kind: CodeUnitKind::Function,
        file: parsed.path.clone(),
        line: 4,
        end_line: 5,
        containing_class: None,
    };
    assert!(is_pragma_no_cover_def(&hidden, &parsed_ref));
    assert!(is_pragma_no_cover_def(&visible, &parsed_ref));
    let mut unreferenced = Vec::new();
    deprioritize_pragma_no_cover_coverage(
        &[hidden.clone(), visible.clone()],
        &mut unreferenced,
        &Vec::new(),
        &parsed_ref,
    );
    assert_eq!(unreferenced.len(), 2);
    let per_test: PerTestUsage = vec![(
        PathBuf::from("tests/test_x.py"),
        vec![("test_x".into(), HashSet::from(["hidden".into()]))],
    )];
    let mut unreferenced2 = Vec::new();
    deprioritize_pragma_no_cover_coverage(
        &[hidden],
        &mut unreferenced2,
        &per_test,
        &parsed_ref,
    );
    assert!(unreferenced2.is_empty());
}
