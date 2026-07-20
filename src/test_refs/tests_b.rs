#![allow(clippy::let_unit_value)]

use std::path::Path;

#[test]
fn test_try_add_py_def_private_skipped() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def _private():\n    pass\ndef __init__(self):\n    pass\ndef test_foo():\n    pass\ndef normal():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let mut defs = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "function_definition" {
            super::collect::try_add_py_def(
                child,
                src,
                Path::new("mod.py"),
                &mut defs,
                crate::units::CodeUnitKind::Function,
                None,
            );
        }
    }
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(!names.contains(&"_private"), "private functions skipped");
    assert!(names.contains(&"__init__"), "__init__ is allowed");
    assert!(!names.contains(&"test_foo"), "test_ functions skipped");
    assert!(names.contains(&"normal"), "normal functions included");
}

// ---------------------------------------------------------------------------
// collect.rs: insert_identifier
// ---------------------------------------------------------------------------

#[test]
fn test_insert_identifier_captures_name() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "x\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let expr_stmt = root.child(0).unwrap();
    let ident = expr_stmt.child(0).unwrap();
    assert_eq!(ident.kind(), "identifier");
    let mut refs = std::collections::HashSet::new();
    super::collect::insert_identifier(ident, src, &mut refs);
    assert!(refs.contains("x"));
}

// ---------------------------------------------------------------------------
// collect.rs: collect_usage_refs_in_scope
// ---------------------------------------------------------------------------

#[test]
fn test_collect_usage_refs_in_scope_gathers_calls_and_identifiers() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def test_it():\n    foo()\n    bar.baz()\n    x = helper\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let func_def = root.child(0).unwrap();
    let body = func_def.child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    super::collect::collect_usage_refs_in_scope(body, src, &mut refs);
    assert!(refs.contains("foo"), "direct call captured");
    assert!(refs.contains("baz"), "attribute call captured");
    assert!(refs.contains("helper"), "bare identifier captured");
}

// ---------------------------------------------------------------------------
// collect.rs: collect_class_test_methods
// ---------------------------------------------------------------------------

#[test]
fn test_collect_class_test_methods_extracts_test_methods() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class TestFoo:\n    def test_one(self):\n        run()\n    def helper(self):\n        pass\n    def test_two(self):\n        go()\n";
    let tree = parser.parse(src, None).unwrap();
    let class_node = tree.root_node().child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    let mut out = Vec::new();
    super::collect::collect_class_test_methods(body, src, "TestFoo", &mut out);
    let ids: Vec<&str> = out.iter().map(|(id, _, _)| id.as_str()).collect();
    assert!(ids.contains(&"TestFoo::test_one"));
    assert!(ids.contains(&"TestFoo::test_two"));
    assert!(!ids.iter().any(|id| id.contains("helper")));
    let (_, refs, call_refs) = out
        .iter()
        .find(|(id, _, _)| id == "TestFoo::test_one")
        .unwrap();
    assert!(refs.contains("run"));
    assert!(call_refs.contains("run"));
}

// ---------------------------------------------------------------------------
// collect.rs: collect_test_functions_with_refs
// ---------------------------------------------------------------------------

#[test]
fn test_collect_test_functions_with_refs_top_level_and_class() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def test_alpha():\n    do_alpha()\n\nclass TestBeta:\n    def test_beta_one(self):\n        do_beta()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut out = Vec::new();
    super::collect::collect_test_functions_with_refs(tree.root_node(), src, "", &mut out);
    let ids: Vec<&str> = out.iter().map(|(id, _, _)| id.as_str()).collect();
    assert!(ids.contains(&"test_alpha"));
    assert!(ids.contains(&"TestBeta::test_beta_one"));
}

// ---------------------------------------------------------------------------
// collect.rs: collect_all_test_file_data
// ---------------------------------------------------------------------------

#[test]
fn test_collect_all_test_file_data_imports_calls_decorators() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from mymod import helper\nimport pytest\n\n@pytest.mark.slow\ndef test_x():\n    helper()\n";
    let tree = parser.parse(src, None).unwrap();
    let mut test_refs = std::collections::HashSet::new();
    let mut usage_refs = std::collections::HashSet::new();
    let mut call_refs = std::collections::HashSet::new();
    let mut import_bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();
    super::collect::collect_all_test_file_data(
        tree.root_node(),
        src,
        &mut test_refs,
        &mut usage_refs,
        &mut call_refs,
        &mut import_bindings,
        &mut alias_bindings,
    );
    assert!(
        test_refs.contains("helper"),
        "import name captured in test_refs"
    );
    assert!(
        test_refs.contains("mymod"),
        "module name captured in test_refs"
    );
    assert!(test_refs.contains("pytest"), "import captured in test_refs");
    assert!(usage_refs.contains("helper"), "call captured in usage_refs");
    assert!(call_refs.contains("helper"), "call captured in call_refs");
    assert!(
        import_bindings.get("mymod").unwrap().contains("helper"),
        "import binding recorded"
    );
}

#[test]
fn test_collect_all_test_file_data_records_type_and_alias_import_refs() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from pkg.mod import Thing as Alias, other.deep\nfrom .local import Hidden\n\ndef test_x(value: pkg.Type):\n    item: Alias = Alias()\n    return other.deep.call(item)\n";
    let tree = parser.parse(src, None).unwrap();
    let mut test_refs = std::collections::HashSet::new();
    let mut usage_refs = std::collections::HashSet::new();
    let mut call_refs = std::collections::HashSet::new();
    let mut import_bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();

    super::collect::collect_all_test_file_data(
        tree.root_node(),
        src,
        &mut test_refs,
        &mut usage_refs,
        &mut call_refs,
        &mut import_bindings,
        &mut alias_bindings,
    );

    assert!(test_refs.contains("deep"));
    assert!(usage_refs.contains("Type"));
    assert!(call_refs.contains("call"));
    assert!(import_bindings.get("pkg.mod").unwrap().contains("Thing"));
    assert_eq!(alias_bindings.get("Alias"), Some(&"Thing".to_string()));
    assert!(
        !import_bindings.contains_key(".local"),
        "relative imports should not create module bindings"
    );
}
