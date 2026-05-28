use super::definitions;
use super::analyze_rust_test_refs;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_trivial_binary_main_rejects_qualified_call_with_unvetted_arguments() {
    let ast: syn::File = syn::parse_str("fn main() { lib::run(compute()); }").unwrap();
    let syn::Item::Fn(f) = &ast.items[0] else {
        panic!("expected fn");
    };
    assert!(
        !definitions::is_trivial_binary_main(f, std::path::Path::new("src/main.rs")),
        "arguments to a qualified call must be analyzed; otherwise real work can hide under a qualified callee"
    );
}

#[test]
fn test_trivial_binary_main_rejects_method_call_with_unvetted_arguments() {
    let ast: syn::File = syn::parse_str("fn main() { x.foo(bar()); }").unwrap();
    let syn::Item::Fn(f) = &ast.items[0] else {
        panic!("expected fn");
    };
    assert!(
        !definitions::is_trivial_binary_main(f, std::path::Path::new("src/main.rs")),
        "method call arguments must be analyzed for trivial-main classification"
    );
}

#[test]
fn test_trivial_main_skipped_in_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let main_path = tmp.path().join("main.rs");
    std::fs::write(&main_path, "fn main() { hello_world::run(); }").unwrap();
    let parsed = parse_rust_file(&main_path).unwrap();
    let analysis = analyze_rust_test_refs(&[&parsed], None);
    assert!(
        !analysis.definitions.iter().any(|d| d.name == "main"),
        "trivial main excluded"
    );

    std::fs::write(&main_path, "fn main() { compute_stuff(); }").unwrap();
    let parsed = parse_rust_file(&main_path).unwrap();
    let analysis = analyze_rust_test_refs(&[&parsed], None);
    assert!(
        analysis.definitions.iter().any(|d| d.name == "main"),
        "nontrivial main included"
    );
}
