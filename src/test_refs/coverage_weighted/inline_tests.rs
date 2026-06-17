use super::*;
use crate::parsing::parse_files;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};

macro_rules! parse_one {
    ($path:expr, $source:expr $(,)?) => {{
        std::fs::write($path, $source).unwrap();
        parse_files(&[$path.to_path_buf()])
            .unwrap()
            .into_iter()
            .flatten()
            .next()
            .unwrap()
    }};
}

macro_rules! empty_analysis {
    ($definitions:expr $(,)?) => {{
        TestRefAnalysis {
            definitions: $definitions,
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        }
    }};
}

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
fn find_def_node_at_line_handles_classes_methods_functions_and_misses() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("nodes.py");
    let parsed = parse_one!(
        &path,
        "class Widget:\n    def method(self):\n        return 1\n\ndef top():\n    return 2\n",
    );
    let root = parsed.tree.root_node();

    assert_eq!(
        find_def_node_at_line(root, 1).unwrap().kind(),
        "class_definition"
    );
    assert_eq!(
        find_def_node_at_line(root, 2).unwrap().kind(),
        "function_definition"
    );
    assert_eq!(
        find_def_node_at_line(root, 5).unwrap().kind(),
        "function_definition"
    );
    assert!(find_def_node_at_line(root, 99).is_none());
}

#[test]
fn test_function_branches_finds_class_and_top_level_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("test_nodes.py");
    let parsed = parse_one!(
        &path,
        "class TestWidget:\n    def test_method(self):\n        if True:\n            assert True\n\ndef test_top():\n    if True:\n        assert True\n",
    );

    assert!(test_function_branches(&parsed, "TestWidget::test_method") > 0);
    assert!(test_function_branches(&parsed, "test_top") > 0);
    assert_eq!(test_function_branches(&parsed, "TestWidget::missing"), 0);
    assert_eq!(test_function_branches(&parsed, "missing"), 0);
}

#[test]
fn weighted_file_pcts_skip_unmatched_or_unlocatable_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("simple.py");
    let parsed = parse_one!(&path, "value = 1\n");
    let missing_path = tmp.path().join("missing.py");
    let analysis = empty_analysis!(vec![
        CodeDefinition {
            name: "ghost".to_string(),
            kind: CodeUnitKind::Function,
            file: parsed.path.clone(),
            line: 99,
            containing_class: None,
        },
        CodeDefinition {
            name: "absent".to_string(),
            kind: CodeUnitKind::Function,
            file: missing_path,
            line: 1,
            containing_class: None,
        },
    ]);

    let weighted = compute_py_weighted_file_pcts(&analysis, &[&parsed]);

    assert!(weighted.is_empty());
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
fn sparse_module_gets_partial_credit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("big.py");
    let body: String = (0..30)
        .map(|i| format!("def f{i}(x: int) -> int:\n    return x + {i}\n"))
        .collect();
    std::fs::write(&module, body).unwrap();
    let test_path = tmp.path().join("test_big.py");
    std::fs::write(
        &test_path,
        "from big import f0\n\ndef test_one():\n    assert f0(1) == 2\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let weighted = compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&module).copied().unwrap_or(0);
    assert!(
        pct > 0 && pct < 100,
        "sparse module should get partial credit, got {pct}%"
    );
}
