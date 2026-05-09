use super::CodeUnitVisitor;
use super::extract_rust_code_units;
use crate::rust_parsing::parse_rust_file;
use crate::units::CodeUnitKind;
use std::io::Write;
use syn::visit::Visit;
use tempfile::NamedTempFile;

fn parse_and_extract(code: &str) -> Vec<super::RustCodeUnit> {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    write!(file, "{code}").unwrap();
    let parsed = parse_rust_file(file.path()).expect("should parse");
    extract_rust_code_units(&parsed)
}

#[test]
fn extracts_function() {
    let units = parse_and_extract("fn hello() {}");

    let functions: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Function)
        .collect();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "hello");
}

#[test]
fn extracts_struct_and_methods() {
    let units = parse_and_extract(
        r"
struct Counter { value: i32 }

impl Counter {
    fn new() -> Self { Counter { value: 0 } }
    fn get(&self) -> i32 { self.value }
}
",
    );

    let structs: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Class)
        .collect();
    let methods: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Method)
        .collect();

    assert_eq!(structs.len(), 1);
    assert_eq!(structs[0].name, "Counter");

    assert_eq!(methods.len(), 2);
    assert!(methods.iter().any(|m| m.name == "new"));
    assert!(methods.iter().any(|m| m.name == "get"));
}

#[test]
fn extracts_enum() {
    let units = parse_and_extract("enum Color { Red, Green, Blue }");

    let enums: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Class)
        .collect();
    assert_eq!(enums.len(), 1);
    assert_eq!(enums[0].name, "Color");
}

#[test]
fn includes_module_for_file() {
    let units = parse_and_extract("fn foo() {}");

    let has_module = units.iter().any(|u| u.kind == CodeUnitKind::Module);
    assert!(has_module, "Should have at least one module (the file)");
}

#[test]
fn test_code_unit_visitor_struct() {
    let visitor = CodeUnitVisitor::new("fn foo() {}\n");
    assert!(visitor.source_lines >= 1);
}

#[test]
fn test_visit_item_directly() {
    let file: syn::File = syn::parse_str("fn bar() {}").unwrap();
    let mut visitor = CodeUnitVisitor::new("fn bar() {}\n");
    visitor.visit_item(&file.items[0]);
    assert!(visitor.units.iter().any(|u| u.name == "bar"));
}

#[test]
fn test_estimate_block_lines() {
    let file: syn::File = syn::parse_str("fn f() { let x = 1; let y = 2; }").unwrap();
    if let syn::Item::Fn(func) = &file.items[0] {
        let lines = super::estimate_block_lines(&func.block);
        assert!(lines >= 1);
    }
}
