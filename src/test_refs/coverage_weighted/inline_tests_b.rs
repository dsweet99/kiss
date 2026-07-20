use super::*;
use crate::parsing::parse_files;
use crate::units::CodeUnitKind;

#[test]
fn module_import_surface_credit_can_be_nonzero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(
        &module,
        "def covered(x):\n    if x:\n        return 1\n    return 0\n\ndef sibling():\n    return 2\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import covered\n\ndef test_covered():\n    if covered(True):\n        assert True\n",
    )
    .unwrap();
    let parsed: Vec<_> = parse_files(&[module.clone(), test_path.clone()])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let parsed_by_path: std::collections::HashMap<_, _> =
        parsed.iter().map(|p| (p.path.clone(), p)).collect();
    let parsed_mod = parsed_by_path.get(&module).unwrap();
    let covered = analysis
        .definitions
        .iter()
        .find(|d| d.name == "covered")
        .unwrap();
    let node = find_def_node_at_line(parsed_mod.tree.root_node(), covered.line).unwrap();
    let covering = analysis
        .coverage_map
        .get(&(module.clone(), "covered".into()))
        .map_or(&[][..], std::vec::Vec::as_slice);

    let credit = module_import_surface_credit(
        covered,
        &analysis,
        node,
        &parsed_mod.source,
        covering,
        &parsed_by_path,
    );

    assert!(credit.is_some_and(|value| value > 0.0 && value <= 1.0));
}

#[test]
fn class_import_surface_credit_can_be_nonzero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(
        &module,
        "class Widget:\n    def covered(self, x):\n        if x:\n            return 1\n        return 0\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import Widget\n\ndef test_widget():\n    w = Widget()\n    if w.covered(True):\n        assert True\n",
    )
    .unwrap();
    let parsed: Vec<_> = parse_files(&[module.clone(), test_path.clone()])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let parsed_by_path: std::collections::HashMap<_, _> =
        parsed.iter().map(|p| (p.path.clone(), p)).collect();
    let parsed_mod = parsed_by_path.get(&module).unwrap();
    let covered = analysis
        .definitions
        .iter()
        .find(|d| d.name == "covered")
        .unwrap();
    let node = find_def_node_at_line(parsed_mod.tree.root_node(), covered.line).unwrap();
    let covering = analysis
        .coverage_map
        .get(&(module.clone(), "Widget".into()))
        .map_or(&[][..], std::vec::Vec::as_slice);

    let credit = class_import_surface_credit(
        covered,
        &analysis,
        node,
        &parsed_mod.source,
        covering,
        &parsed_by_path,
    );

    assert!(credit.is_some_and(|value| value > 0.0 && value <= 1.0));
}

#[test]
fn branch_credit_handles_missing_defs_and_ratios() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("svc.py");
    std::fs::write(
        &module,
        "def branchy(x):\n    if x == 1:\n        return 1\n    if x == 2:\n        return 2\n    return 0\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_svc.py");
    std::fs::write(
        &test_path,
        "from svc import branchy\n\ndef test_branchy():\n    if branchy(1):\n        assert True\n",
    )
    .unwrap();
    let parsed: Vec<_> = parse_files(&[module.clone(), test_path.clone()])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = crate::test_refs::analyze_test_refs(&refs, None);
    let parsed_by_path: std::collections::HashMap<_, _> =
        parsed.iter().map(|p| (p.path.clone(), p)).collect();
    let parsed_mod = parsed_by_path.get(&module).unwrap();
    let branchy = analysis
        .definitions
        .iter()
        .find(|d| d.name == "branchy")
        .unwrap();
    let covering = analysis
        .coverage_map
        .get(&(module.clone(), "branchy".into()))
        .map_or(&[][..], std::vec::Vec::as_slice);

    let partial =
        definition_branch_credit(branchy, &analysis, parsed_mod, covering, &parsed_by_path);
    assert!(partial > 0.0 && partial < 1.0);

    let missing = CodeDefinition {
        file: module.clone(),
        name: "ghost".to_string(),
        line: 999,
        kind: CodeUnitKind::Function,
        containing_class: None,
    };
    assert_eq!(
        definition_branch_credit(&missing, &analysis, parsed_mod, covering, &parsed_by_path),
        1.0
    );
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
