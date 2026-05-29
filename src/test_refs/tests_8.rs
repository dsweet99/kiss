use super::*;
use std::collections::HashMap;

#[test]
fn oi_module_import_credits_protocol_stubs_in_nested_package_layout() {
    use crate::graph::build_dependency_graph;
    use crate::parsing::{create_parser, parse_file};
    use std::fs;
    let dir = tempfile::tempdir().expect("tempdir");
    let iface_path = dir
        .path()
        .join("rope/rope/base/oi/type_hinting/providers/interfaces.py");
    fs::create_dir_all(iface_path.parent().expect("parent")).expect("mkdir");
    fs::write(
        &iface_path,
        "from typing import Protocol\n\nclass IParamProvider(Protocol):\n    pass\n",
    )
    .expect("write iface");
    let test_path = dir.path().join("ropetest/type_hinting_test.py");
    fs::create_dir_all(test_path.parent().expect("parent")).expect("mkdir");
    fs::write(
        &test_path,
        "from rope.base.oi.type_hinting import evaluate\n\ndef test_x():\n    pass\n",
    )
    .expect("write test");
    let mut parser = create_parser().expect("parser");
    let iface_p = parse_file(&mut parser, &iface_path).expect("parse iface");
    let test_p = parse_file(&mut parser, &test_path).expect("parse test");
    let refs = [&iface_p, &test_p];
    let graph = build_dependency_graph(&refs);
    let (_, _, _, import_bindings, _) = collect_refs_parallel_for_coverage_map(&refs);
    assert!(
        import_bindings.contains_key("rope.base.oi.type_hinting"),
        "import_bindings keys: {:?}",
        import_bindings.keys().collect::<Vec<_>>()
    );
    let analysis = analyze_test_refs_for_coverage_map(&refs, Some(&graph));
    let def = analysis
        .definitions
        .iter()
        .find(|d| d.name == "IParamProvider")
        .expect("Protocol stub should be collected");
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(def.file.clone(), file_to_module_suffix(&def.file));
    assert!(
        coverage::is_py_oi_module_import_witnessed(def, &import_bindings, &module_suffixes),
        "suffix={:?}",
        module_suffixes.get(&def.file)
    );
    assert!(
        !analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "IParamProvider"),
        "OI module import should credit IParamProvider"
    );
}
