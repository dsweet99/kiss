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
    assert!(methods.iter().all(|m| !m.trait_impl));
}

#[test]
fn marks_trait_impl_methods() {
    let units = parse_and_extract(
        "pub struct S;\nimpl Default for S { fn default() -> Self { S } }\n",
    );
    let methods: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Method)
        .collect();
    assert_eq!(methods.len(), 1);
    assert!(methods[0].trait_impl);
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

#[test]
fn test_estimate_block_lines_empty_block() {
    let file: syn::File = syn::parse_str("fn empty() {}").unwrap();
    if let syn::Item::Fn(func) = &file.items[0] {
        assert_eq!(super::estimate_block_lines(&func.block), 1);
    }
}

#[test]
fn extracts_inline_module_with_content() {
    let units = parse_and_extract("mod inner { fn nested() {} }");
    let modules: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Module && u.name == "inner")
        .collect();
    assert_eq!(modules.len(), 1);
    assert!(units.iter().any(|u| u.name == "nested"));
}

#[test]
fn extracts_impl_method_parent_type() {
    let units = parse_and_extract(
        "struct Pair(i32, i32);\nimpl Pair { fn sum(&self) -> i32 { self.0 + self.1 } }",
    );
    let methods: Vec<_> = units
        .iter()
        .filter(|u| u.kind == CodeUnitKind::Method && u.name == "sum")
        .collect();
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].parent_type.as_deref(), Some("Pair"));
}

#[test]
fn visit_item_handles_non_fn_items() {
    let file: syn::File = syn::parse_str("const X: i32 = 1;\nuse std::io;\nfn bar() {}\n").unwrap();
    let mut visitor = CodeUnitVisitor::new("const X: i32 = 1;\nuse std::io;\nfn bar() {}\n");
    for item in &file.items {
        visitor.visit_item(item);
    }
    assert!(visitor.units.iter().any(|u| u.name == "bar"));
}

#[test]
fn nested_fn_in_block_increments_line_estimate() {
    let units = parse_and_extract(
        "fn outer() {\n    fn inner() {\n        let x = 1;\n        let y = 2;\n    }\n}",
    );
    let outer: Vec<_> = units
        .iter()
        .filter(|u| u.name == "outer" && u.kind == CodeUnitKind::Function)
        .collect();
    assert_eq!(outer.len(), 1);
    assert!(outer[0].end_line >= outer[0].start_line);
}

#[test]
fn empty_mod_without_content_not_recorded() {
    let units = parse_and_extract("mod empty;\nfn foo() {}\n");
    assert!(
        !units
            .iter()
            .any(|u| u.name == "empty" && u.kind == CodeUnitKind::Module)
    );
    assert!(units.iter().any(|u| u.name == "foo"));
}
