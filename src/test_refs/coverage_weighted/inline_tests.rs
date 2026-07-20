use super::*;
use crate::parsing::parse_files;
use crate::units::CodeUnitKind;

#[test]
fn direct_weighted_helpers_via_fixtures() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(
        &module,
        "class Widget:\n    def ok(self):\n        return 1\n    def heavy(self, n: int) -> int:\n        total = 0\n        for i in range(20):\n            total += i * n\n        return total\n\ndef orphan(n: int) -> int:\n    return n + 1\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import Widget, orphan\n\ndef test_widget():\n    w = Widget()\n    assert w.ok() == 1\n    assert orphan(1) == 2\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let weighted = compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&module).copied().unwrap_or(100);
    assert!(
        pct < 100,
        "expected partial credit on mixed module, got {pct}%"
    );
    assert!(
        analysis
            .definitions
            .iter()
            .any(|d| d.kind == CodeUnitKind::Class),
        "fixture should include class defs"
    );
}

#[test]
fn direct_import_surface_credit_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("mod.py");
    std::fs::write(
        &module,
        "class Widget:\n    def ok(self):\n        return 1\n    def heavy(self, n: int) -> int:\n        return n + 1\n\ndef orphan(n: int) -> int:\n    return n\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_mod.py");
    std::fs::write(
        &test_path,
        "from mod import Widget, orphan\n\ndef test_all():\n    w = Widget()\n    assert w.ok() == 1\n    assert orphan(0) == 0\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let parsed_by_path: std::collections::HashMap<_, _> =
        parsed.iter().map(|p| (p.path.clone(), p)).collect();
    let empty_cover: &[(PathBuf, String)] = &[];
    let heavy = analysis
        .definitions
        .iter()
        .find(|d| d.name == "heavy")
        .unwrap();
    let parsed_mod = parsed_by_path.get(&module).unwrap();
    let root = parsed_mod.tree.root_node();
    let node = find_def_node_at_line(root, heavy.line).expect("heavy node");
    let cls_cover = class_covering_tests(
        &analysis,
        &std::collections::HashSet::new(),
        &module,
        "Widget",
    );
    let cls_slice = cls_cover.unwrap_or(empty_cover);
    let heavy_credit = class_import_surface_credit(
        heavy,
        &analysis,
        node,
        &parsed_mod.source,
        cls_slice,
        &parsed_by_path,
    );
    assert_eq!(
        heavy_credit,
        Some(0.0),
        "unreferenced heavy method with no branch witness in covering tests gets zero credit"
    );
    let orphan = analysis
        .definitions
        .iter()
        .find(|d| d.name == "orphan")
        .unwrap();
    let orphan_node = find_def_node_at_line(root, orphan.line).expect("orphan node");
    let cover_slice = analysis
        .coverage_map
        .get(&(module.clone(), "orphan".into()))
        .map_or(empty_cover, std::vec::Vec::as_slice);
    let orphan_credit = module_import_surface_credit(
        orphan,
        &analysis,
        orphan_node,
        &parsed_mod.source,
        cover_slice,
        &parsed_by_path,
    );
    assert_eq!(
        orphan_credit,
        Some(0.0),
        "orphan with call witness but no covering tests should get zero module credit"
    );
}

#[test]
fn direct_class_covering_and_call_witness() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("mod.py");
    std::fs::write(
        &module,
        "class Widget:\n    def ok(self):\n        return 1\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_mod.py");
    std::fs::write(
        &test_path,
        "from mod import Widget\n\ndef test_all():\n    assert Widget().ok() == 1\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    assert!(py_call_witness(&analysis, &module, "ok"));
    let covering = class_covering_tests(&analysis, &unref_set, &module, "Widget");
    assert!(
        covering.is_some(),
        "Widget should have covering tests when ok() is witnessed"
    );
    assert!(
        !covering.unwrap().is_empty(),
        "covering tests should list at least one test file"
    );
}

#[test]
fn unreferenced_class_has_no_import_surface_covering_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("mod.py");
    std::fs::write(
        &module,
        "class Widget:\n    def ok(self):\n        return 1\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_mod.py");
    std::fs::write(&test_path, "def test_unrelated():\n    assert True\n").unwrap();
    let paths = vec![module.clone(), test_path];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();

    assert!(unref_set.contains(&(&module, "Widget")));
    assert!(class_covering_tests(&analysis, &unref_set, &module, "Widget").is_none());
}

#[test]
fn py_init_marker_empty_package_init_is_hundred() {
    let tmp = tempfile::TempDir::new().unwrap();
    let init = tmp.path().join("pkg/__init__.py");
    std::fs::create_dir_all(init.parent().unwrap()).unwrap();
    std::fs::write(&init, "\"\"\"Package marker.\"\"\"\n").unwrap();
    let parsed = parse_files(std::slice::from_ref(&init))
        .unwrap()
        .into_iter()
        .flatten()
        .next()
        .unwrap();
    assert_eq!(super::py_init_marker_pct(&parsed), 100);
}

#[test]
fn py_init_marker_reexport_barrel_is_zero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let init = tmp.path().join("pkg/__init__.py");
    std::fs::create_dir_all(init.parent().unwrap()).unwrap();
    std::fs::write(&init, "from .core import run\n").unwrap();
    let parsed = parse_files(std::slice::from_ref(&init))
        .unwrap()
        .into_iter()
        .flatten()
        .next()
        .unwrap();
    assert_eq!(super::py_init_marker_pct(&parsed), 0);
}

#[test]
fn py_init_marker_ignores_non_init_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("pkg/module.py");
    std::fs::create_dir_all(module.parent().unwrap()).unwrap();
    std::fs::write(&module, "\"\"\"Not an init marker.\"\"\"\n").unwrap();
    let parsed = parse_files(std::slice::from_ref(&module))
        .unwrap()
        .into_iter()
        .flatten()
        .next()
        .unwrap();

    assert_eq!(super::py_init_marker_pct(&parsed), 0);
}

#[test]
fn weighted_coverage_skips_definitions_without_parsed_file() {
    let analysis = TestRefAnalysis {
        definitions: vec![CodeDefinition {
            file: PathBuf::from("missing.py"),
            name: "missing".to_string(),
            line: 1,
            kind: CodeUnitKind::Function,
            containing_class: None,
        }],
        test_references: Default::default(),
        call_references: Default::default(),
        unreferenced: Vec::new(),
        coverage_map: Default::default(),
    };

    assert!(compute_py_weighted_file_pcts(&analysis, &[]).is_empty());
}

#[test]
fn class_method_test_ids_find_nested_test_methods() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(&module, "def value():\n    return 1\n").unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import value\n\nclass TestValue:\n    def test_value(self):\n        if value() == 1:\n            assert True\n",
    )
    .unwrap();
    let parsed: Vec<_> = parse_files(&[module.clone(), test_path.clone()])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let test_file = parsed.iter().find(|p| p.path == test_path).unwrap();

    assert_eq!(
        test_function_branches(test_file, "TestValue::test_value"),
        1
    );
    assert_eq!(test_function_branches(test_file, "TestValue::missing"), 0);
}

#[test]
fn top_level_test_id_and_class_definition_lookup_are_supported() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(
        &module,
        "class Widget:\n    pass\n\ndef value():\n    return 1\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import value\n\ndef test_value():\n    if value() == 1:\n        assert True\n",
    )
    .unwrap();
    let parsed: Vec<_> = parse_files(&[module.clone(), test_path.clone()])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let module_file = parsed.iter().find(|p| p.path == module).unwrap();
    let test_file = parsed.iter().find(|p| p.path == test_path).unwrap();

    assert_eq!(test_function_branches(test_file, "test_value"), 1);
    assert!(find_def_node_at_line(module_file.tree.root_node(), 1).is_some());
    assert!(find_def_node_at_line(module_file.tree.root_node(), 999).is_none());
    assert!(is_named_class(
        module_file.tree.root_node().named_child(0).unwrap(),
        &module_file.source,
        "Widget"
    ));
    assert_eq!(test_function_branches(test_file, "missing_test"), 0);
}
