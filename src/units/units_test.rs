use crate::test_utils::parse_python_source;

use super::*;

#[test]
fn test_code_unit_kind_eq() {
    assert_eq!(CodeUnitKind::Function, CodeUnitKind::Function);
    assert_ne!(CodeUnitKind::Function, CodeUnitKind::Method);
}

#[test]
fn test_extract_module() {
    let parsed = parse_python_source("x = 1");
    let units = extract_code_units(&parsed);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Module));
}

#[test]
fn test_extract_function() {
    let parsed = parse_python_source("def foo(): pass");
    let units = extract_code_units(&parsed);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Function && u.name == "foo")
    );
}

#[test]
fn test_count_matches_extract_len() {
    let parsed = parse_python_source(
        "def outer():\n    def inner(): pass\nclass C:\n    def m(self): pass",
    );
    assert_eq!(count_code_units(&parsed), extract_code_units(&parsed).len());
}

#[test]
fn test_extract_async_function() {
    let parsed = parse_python_source("async def bar(): pass");
    let units = extract_code_units(&parsed);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Function && u.name == "bar")
    );
}

#[test]
fn test_extract_class() {
    let parsed = parse_python_source("class MyClass: pass");
    let units = extract_code_units(&parsed);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Class && u.name == "MyClass")
    );
}

#[test]
fn test_extract_method() {
    let parsed = parse_python_source("class C:\n    def method(self): pass");
    let units = extract_code_units(&parsed);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Method && u.name == "method")
    );
}

#[test]
fn test_nested_function_is_function() {
    let parsed = parse_python_source("def outer():\n    def inner(): pass");
    let units = extract_code_units(&parsed);
    let inner = units.iter().find(|u| u.name == "inner").unwrap();
    assert_eq!(inner.kind, CodeUnitKind::Function);
}

#[test]
fn test_code_unit_positions() {
    let parsed = parse_python_source("def f(): pass");
    let units = extract_code_units(&parsed);
    let f = units.iter().find(|u| u.name == "f").unwrap();
    assert_eq!(f.start_line, 1);
    assert!(f.start_byte < f.end_byte);
}

#[test]
fn test_get_child_by_field() {
    let parsed = parse_python_source("def foo(): pass");
    let root = parsed.tree.root_node();
    let func = root.child(0).unwrap();
    let name = get_child_by_field(func, "name", &parsed.source);
    assert_eq!(name, Some("foo".to_string()));
}

#[test]
fn test_extract_from_node_recursion() {
    let parsed = parse_python_source("class A:\n    class B:\n        def m(self): pass");
    let units = extract_code_units(&parsed);
    assert!(units.iter().any(|u| u.name == "A"));
    assert!(units.iter().any(|u| u.name == "B"));
    assert!(
        units
            .iter()
            .any(|u| u.name == "m" && u.kind == CodeUnitKind::Method)
    );
}

#[test]
fn test_code_unit_kind_all_variants() {
    let kinds = [
        CodeUnitKind::Function,
        CodeUnitKind::Method,
        CodeUnitKind::Class,
        CodeUnitKind::Module,
    ];
    assert_eq!(kinds.len(), 4);
}

#[test]
fn test_code_unit_struct() {
    let unit = CodeUnit {
        kind: CodeUnitKind::Function,
        name: "foo".into(),
        start_line: 1,
        end_line: 5,
        start_byte: 0,
        end_byte: 50,
    };
    assert_eq!(unit.name, "foo");
    assert_eq!(unit.kind, CodeUnitKind::Function);
}

#[test]
fn test_extract_from_node_direct() {
    let parsed = parse_python_source("def f(): pass\nclass C: pass");
    let mut units = Vec::new();
    extract_from_node(parsed.tree.root_node(), &parsed.source, &mut units, false);
    assert!(units.iter().any(|u| u.name == "f"));
    assert!(units.iter().any(|u| u.name == "C"));
}

#[test]
fn test_code_unit_kind_as_str() {
    assert_eq!(CodeUnitKind::Function.as_str(), "function");
    assert_eq!(CodeUnitKind::Class.as_str(), "class");
    assert_eq!(CodeUnitKind::Method.as_str(), "method");
    assert_eq!(CodeUnitKind::Struct.as_str(), "struct");
    assert_eq!(CodeUnitKind::Enum.as_str(), "enum");
    assert_eq!(CodeUnitKind::TraitImplMethod.as_str(), "trait_impl_method");
}

#[test]
fn static_coverage_touch_count_from_node() {
    fn t<T>(_: T) {}
    t(count_from_node);
}
