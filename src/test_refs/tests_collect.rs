use std::collections::{HashMap, HashSet};

#[test]
fn test_collect_test_functions_with_refs_for_coverage_map_class() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class TestFoo:\n    def test_one(self):\n        run()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "TestFoo::test_one");
    assert!(out[0].1.contains("run"));
}

#[test]
fn test_collect_all_test_file_data_for_coverage_map_smoke() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import foo\n\ndef test_x():\n    foo.bar()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut usage = HashSet::new();
    super::collect_all_test_file_data_for_coverage_map(
        tree.root_node(),
        src,
        &mut HashSet::new(),
        &mut usage,
        &mut HashMap::new(),
    );
    assert!(usage.contains("bar"));
}

#[test]
fn test_collect_all_test_file_data_import_and_type_nodes() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import os\n\ndef test_y():\n    os.path()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut test_refs = HashSet::new();
    let mut usage = HashSet::new();
    super::collect_all_test_file_data_for_coverage_map(
        tree.root_node(),
        src,
        &mut test_refs,
        &mut usage,
        &mut HashMap::new(),
    );
    assert!(test_refs.contains("os"));
    assert!(usage.contains("path"));
    let src3 = "from typing import Optional\n\ndef test_z(x: Optional[int]):\n    pass\n";
    let tree3 = parser.parse(src3, None).unwrap();
    let mut tr3 = HashSet::new();
    super::collect_all_test_file_data(
        tree3.root_node(),
        src3,
        &mut tr3,
        &mut HashSet::new(),
        &mut HashMap::new(),
    );
    assert!(tr3.contains("Optional"));
}

#[test]
fn test_process_test_file_ast_node_bare_identifier() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src4 = "def test_bare():\n    bare_name\n";
    let tree4 = parser.parse(src4, None).unwrap();
    let body4 = tree4
        .root_node()
        .child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut direct = HashSet::new();
    super::process_test_file_ast_node(
        body4,
        src4,
        &mut HashSet::new(),
        &mut direct,
        &mut HashMap::new(),
        true,
    );
    assert!(direct.contains("bare_name"));
}

#[test]
fn test_collect_all_test_file_data_gate_mode() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import foo\n\ndef test_x():\n    foo: int\n    foo()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut test_refs = HashSet::new();
    let mut usage = HashSet::new();
    super::collect_all_test_file_data(
        tree.root_node(),
        src,
        &mut test_refs,
        &mut usage,
        &mut HashMap::new(),
    );
    assert!(test_refs.contains("foo"));
    assert!(usage.contains("foo"));
}

#[test]
fn test_collect_calibration_async_and_nested() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "async def test_async():\n    await go()\n\ndef helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].1.contains("go"));
}

#[test]
fn test_collect_usage_refs_in_scope_gate_decorator_and_type() {
    use crate::parsing::create_parser;
    use super::collect::{
        collect_usage_refs_in_scope, collect_usage_refs_in_scope_with_mode, UsageRefMode,
    };
    let mut parser = create_parser().unwrap();
    let src = "def test_x():\n    x: Foo\n    call()\n";
    let tree = parser.parse(src, None).unwrap();
    let body = tree
        .root_node()
        .child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut gate = HashSet::new();
    collect_usage_refs_in_scope(body, src, &mut gate);
    assert!(gate.contains("call"));
    assert!(gate.contains("Foo"));
    let mut cal = HashSet::new();
    collect_usage_refs_in_scope_with_mode(body, src, &mut cal, UsageRefMode::CalibrationWitness);
    assert!(cal.contains("call"));
    assert!(!cal.contains("Foo"));
}

#[test]
fn test_collect_test_functions_skips_non_test_helpers() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def helper():\n    pass\n\ndef test_ok():\n    helper()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].1.contains("helper"));
}

#[test]
fn test_collect_class_test_methods_async() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class TestFoo:\n    async def test_async(self):\n        await go()\n";
    let tree = parser.parse(src, None).unwrap();
    let body = tree
        .root_node()
        .child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut out = Vec::new();
    super::collect::collect_class_test_methods(body, src, "TestFoo", &mut out);
    assert_eq!(out.len(), 1);
    assert!(out[0].1.contains("go"));
}

#[test]
fn test_collect_test_functions_with_decorated_definition() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "@deco\ndef test_wrapped():\n    wrapped()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].1.contains("wrapped"));
}

#[test]
fn test_collect_test_functions_with_prefix_and_non_test_class() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class Helper:\n    def test_in_helper(self):\n        h()\n\ndef test_top():\n    top()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "pkg",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "pkg::test_top");
    assert!(out[0].1.contains("top"));
}

#[test]
fn test_collect_class_test_methods_with_nested_prefix() {
    use crate::parsing::create_parser;
    use super::collect::{collect_class_test_methods_with_mode, UsageRefMode};
    let mut parser = create_parser().unwrap();
    let src = "class TestFoo:\n    def test_one(self):\n        run()\n";
    let tree = parser.parse(src, None).unwrap();
    let body = tree
        .root_node()
        .child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut out = Vec::new();
    collect_class_test_methods_with_mode(
        body,
        src,
        "outer::TestFoo",
        &mut out,
        UsageRefMode::CalibrationWitness,
    );
    assert_eq!(out[0].0, "outer::TestFoo::test_one");
}
