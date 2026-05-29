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
fn test_collect_test_functions_unittest_suffix_class() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import unittest\n\nclass ProjectTest(unittest.TestCase):\n    def test_observer(self):\n        FilteredResourceObserver()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "ProjectTest::test_observer");
    assert!(out[0].1.contains("FilteredResourceObserver"));
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

#[test]
fn test_calibration_witness_collects_assert_raises_exception_type() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = r#"
class TestProj(unittest.TestCase):
    def test_missing(self):
        with self.assertRaises(ResourceNotFoundError):
            raise ResourceNotFoundError()
"#;
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_for_coverage_map(
        tree.root_node(),
        src,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert!(
        out[0].1.contains("ResourceNotFoundError"),
        "assertRaises exception type should witness: {:?}",
        out[0].1
    );
}

#[test]
fn test_collect_test_functions_with_refs_mode_gate() {
    use crate::parsing::create_parser;
    use super::collect::{collect_test_functions_with_refs, collect_test_functions_with_refs_mode, UsageRefMode};
    let mut parser = create_parser().unwrap();
    let src = "def test_gate():\n    typed: Foo\n    call()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut gate = Vec::new();
    collect_test_functions_with_refs(tree.root_node(), src, "", &mut gate);
    assert!(gate[0].1.contains("Foo"));
    let mut cal = Vec::new();
    collect_test_functions_with_refs_mode(
        tree.root_node(),
        src,
        "",
        &mut cal,
        UsageRefMode::CalibrationWitness,
    );
    assert!(cal[0].1.contains("call"));
    assert!(!cal[0].1.contains("Foo"));
}

#[test]
fn test_collect_import_modules_and_alias_map() {
    use crate::parsing::create_parser;
    use super::collect::{
        collect_py_import_alias_map, extract_import_statement_modules, UsageRefMode,
    };
    let mut parser = create_parser().unwrap();
    let src = "import os\nfrom pkg import foo as bar\n\ndef test_x():\n    with pytest.raises(MyError):\n        pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let import_node = root
        .children(&mut root.walk())
        .find(|n| n.kind() == "import_statement")
        .expect("import");
    let mut bindings = HashMap::new();
    extract_import_statement_modules(import_node, src, &mut bindings);
    assert!(bindings.contains_key("os"));
    let mut aliases = HashMap::new();
    collect_py_import_alias_map(root, src, &mut aliases);
    assert_eq!(aliases.get("bar").map(String::as_str), Some("foo"));
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs_mode(
        root,
        src,
        "",
        &mut out,
        UsageRefMode::CalibrationWitness,
    );
    assert!(out[0].1.contains("MyError"));
}

#[test]
fn test_collect_definitions_oi_protocol_stub() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from typing import Protocol\n\nclass IProvider(Protocol):\n    def run(self) -> None: ...\n";
    let tree = parser.parse(src, None).unwrap();
    let file = std::path::Path::new("pkg/base/oi/type_hinting/interfaces.py");
    let mut defs = Vec::new();
    super::collect_definitions(tree.root_node(), src, file, &mut defs, false, None);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "IProvider");
    assert!(defs[0].end_line <= defs[0].line + 2);

    let mut parser2 = create_parser().unwrap();
    let src2 = "class Metric:\n    pass\n";
    let tree2 = parser2.parse(src2, None).unwrap();
    let evaluate = std::path::Path::new("pkg/base/oi/evaluate.py");
    let mut defs2 = Vec::new();
    super::collect_definitions(tree2.root_node(), src2, evaluate, &mut defs2, false, None);
    assert_eq!(defs2.len(), 1);
    assert_eq!(defs2[0].end_line, defs2[0].line + 1);

    let mut parser3 = create_parser().unwrap();
    let tree3 = parser3.parse(src, None).unwrap();
    let mut defs3 = Vec::new();
    super::collect_definitions(
        tree3.root_node(),
        src,
        std::path::Path::new("pkg/base/core.py"),
        &mut defs3,
        false,
        None,
    );
    assert!(defs3.is_empty());
}

#[test]
fn test_collect_raises_and_call_tail_branches() {
    use crate::parsing::create_parser;
    use super::collect::{collect_usage_refs_in_scope_with_mode, UsageRefMode};
    let mut parser = create_parser().unwrap();
    let src = "def test_a():\n    other()\n    with pytest.raises(ValueError):\n        pass\n    with self.assertRaises(CustomError):\n        pass\n";
    let tree = parser.parse(src, None).unwrap();
    let body = tree
        .root_node()
        .child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut refs = HashSet::new();
    collect_usage_refs_in_scope_with_mode(body, src, &mut refs, UsageRefMode::CalibrationWitness);
    assert!(refs.contains("ValueError"));
    assert!(refs.contains("CustomError"));
    assert!(refs.contains("other"));
}
