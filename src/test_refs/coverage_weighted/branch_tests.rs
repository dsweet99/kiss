use super::*;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

macro_rules! parse_one {
    ($path:expr, $source:expr $(,)?) => {{
        std::fs::write($path, $source).unwrap();
        let mut parser = crate::parsing::create_parser().unwrap();
        crate::parsing::parse_file(&mut parser, $path).unwrap()
    }};
}

#[test]
fn module_import_surface_credit_uses_test_branch_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    let test_path = tmp.path().join("test_svc.py");
    let parsed_module = parse_one!(
        &module,
        "def target(n: int) -> int:\n    if n:\n        return 1\n    return 0\n",
    );
    let parsed_test = parse_one!(
        &test_path,
        "def test_target(flag):\n    if flag:\n        target(1)\n    else:\n        target(0)\n",
    );
    let def = CodeDefinition {
        name: "target".to_string(),
        kind: CodeUnitKind::Function,
        file: module.clone(),
        line: 1,
        containing_class: None,
    };
    let analysis = TestRefAnalysis {
        definitions: vec![def.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::from([(
            (module.clone(), "target".to_string()),
            vec![(test_path.clone(), "test_target".to_string())],
        )]),
    };
    let parsed_by_path = HashMap::from([
        (module.clone(), &parsed_module),
        (test_path.clone(), &parsed_test),
    ]);
    let root = parsed_module.tree.root_node();
    let node = find_def_node_at_line(root, 1).expect("target node");
    let covering = analysis
        .coverage_map
        .get(&(module, "target".to_string()))
        .unwrap();

    let credit = module_import_surface_credit(
        &def,
        &analysis,
        node,
        &parsed_module.source,
        covering,
        &parsed_by_path,
    );

    assert!(credit.is_some_and(|v| v > 0.0 && v <= 1.0));
}

#[test]
fn class_import_surface_credit_uses_class_covering_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    let test_path = tmp.path().join("test_svc.py");
    let parsed_module = parse_one!(
        &module,
        "class Widget:\n    def used(self):\n        return 1\n    def missing(self, n):\n        if n:\n            return 1\n        return 0\n",
    );
    let parsed_test = parse_one!(
        &test_path,
        "class TestWidget:\n    def test_used(self, flag):\n        if flag:\n            Widget().used()\n        else:\n            Widget().used()\n",
    );
    let class_def = CodeDefinition {
        name: "Widget".to_string(),
        kind: CodeUnitKind::Class,
        file: module.clone(),
        line: 1,
        containing_class: None,
    };
    let method_def = CodeDefinition {
        name: "missing".to_string(),
        kind: CodeUnitKind::Method,
        file: module.clone(),
        line: 4,
        containing_class: Some("Widget".to_string()),
    };
    let analysis = TestRefAnalysis {
        definitions: vec![class_def.clone(), method_def.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: vec![method_def.clone()],
        coverage_map: HashMap::from([(
            (module.clone(), "Widget".to_string()),
            vec![(test_path.clone(), "TestWidget::test_used".to_string())],
        )]),
    };
    let unref_set: HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    let parsed_by_path = HashMap::from([
        (module.clone(), &parsed_module),
        (test_path.clone(), &parsed_test),
    ]);
    let root = parsed_module.tree.root_node();
    let node = find_def_node_at_line(root, method_def.line).expect("method node");
    let covering = class_covering_tests(&analysis, &unref_set, &module, "Widget").unwrap();

    let credit = class_import_surface_credit(
        &method_def,
        &analysis,
        node,
        &parsed_module.source,
        covering,
        &parsed_by_path,
    );

    assert!(credit.is_some_and(|v| v > 0.0 && v <= 1.0));
    assert!(class_covering_tests(&analysis, &unref_set, &module, "Missing").is_none());
}

#[test]
fn definition_branch_credit_reaches_full_credit_when_test_has_enough_branches() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    let test_path = tmp.path().join("test_svc.py");
    let parsed_module = parse_one!(
        &module,
        "def target(n):\n    if n:\n        return 1\n    return 0\n",
    );
    let parsed_test = parse_one!(
        &test_path,
        "def test_target(flag):\n    if flag:\n        target(1)\n    else:\n        target(0)\n",
    );
    let def = CodeDefinition {
        name: "target".to_string(),
        kind: CodeUnitKind::Function,
        file: module.clone(),
        line: 1,
        containing_class: None,
    };
    let analysis = TestRefAnalysis {
        definitions: vec![def.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::from([(
            (module, "target".to_string()),
            vec![(test_path.clone(), "test_target".to_string())],
        )]),
    };
    let parsed_by_path = HashMap::from([(test_path.clone(), &parsed_test)]);
    let covering = vec![(test_path, "test_target".to_string())];

    let credit =
        definition_branch_credit(&def, &analysis, &parsed_module, &covering, &parsed_by_path);

    assert_eq!(credit, 1.0);
}

#[test]
fn class_covering_tests_returns_none_for_unreferenced_class() {
    let file = PathBuf::from("svc.py");
    let class_def = CodeDefinition {
        name: "Widget".to_string(),
        kind: CodeUnitKind::Class,
        file: file.clone(),
        line: 1,
        containing_class: None,
    };
    let analysis = TestRefAnalysis {
        definitions: vec![class_def.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: vec![class_def.clone()],
        coverage_map: HashMap::from([(
            (file.clone(), "Widget".to_string()),
            vec![(PathBuf::from("test_svc.py"), "test_widget".to_string())],
        )]),
    };
    let unref_set: HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();

    assert!(class_covering_tests(&analysis, &unref_set, &file, "Widget").is_none());
}

#[test]
fn definition_branch_credit_handles_missing_and_unwitnessed_defs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    let parsed_module = parse_one!(&module, "def target():\n    return 1\n");
    let missing_def = CodeDefinition {
        name: "ghost".to_string(),
        kind: CodeUnitKind::Function,
        file: module.clone(),
        line: 99,
        containing_class: None,
    };
    let target_def = CodeDefinition {
        name: "target".to_string(),
        kind: CodeUnitKind::Function,
        file: module,
        line: 1,
        containing_class: None,
    };
    let analysis = TestRefAnalysis {
        definitions: vec![missing_def.clone(), target_def.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::new(),
    };
    let parsed_by_path = HashMap::new();

    assert_eq!(
        definition_branch_credit(
            &missing_def,
            &analysis,
            &parsed_module,
            &[],
            &parsed_by_path,
        ),
        1.0
    );
    assert_eq!(
        definition_branch_credit(&target_def, &analysis, &parsed_module, &[], &parsed_by_path),
        0.0
    );
}

#[test]
fn weighted_file_pcts_scores_class_definitions_directly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    let parsed_module = parse_one!(
        &module,
        "class Used:\n    pass\n\nclass Missing:\n    pass\n",
    );
    let used = CodeDefinition {
        name: "Used".to_string(),
        kind: CodeUnitKind::Class,
        file: module.clone(),
        line: 1,
        containing_class: None,
    };
    let missing = CodeDefinition {
        name: "Missing".to_string(),
        kind: CodeUnitKind::Class,
        file: module.clone(),
        line: 4,
        containing_class: None,
    };
    let analysis = TestRefAnalysis {
        definitions: vec![used, missing.clone()],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        unreferenced: vec![missing],
        coverage_map: HashMap::from([(
            (module.clone(), "Used".to_string()),
            vec![(PathBuf::from("test_svc.py"), "test_used".to_string())],
        )]),
    };

    let weighted = compute_py_weighted_file_pcts(&analysis, &[&parsed_module]);

    assert_eq!(weighted.get(&module), Some(&50));
}
