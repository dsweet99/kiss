use super::*;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_impl_method_covered_when_type_referenced() {
    let tmp = tempfile::TempDir::new().unwrap();

    let alpha_path = tmp.path().join("alpha.rs");
    std::fs::write(
        &alpha_path,
        "pub struct Foo {}\nimpl Foo {\n    pub fn new() -> Self { Foo {} }\n}\n",
    )
    .unwrap();

    let beta_path = tmp.path().join("beta.rs");
    std::fs::write(
        &beta_path,
        "pub struct Bar {}\nimpl Bar {\n    pub fn new() -> Self { Bar {} }\n}\n",
    )
    .unwrap();

    let test_path = tmp.path().join("test_alpha.rs");
    std::fs::write(&test_path, "fn t() { let _f = Foo::new(); }").unwrap();

    let parsed_alpha = parse_rust_file(&alpha_path).unwrap();
    let parsed_beta = parse_rust_file(&beta_path).unwrap();
    let parsed_test = parse_rust_file(&test_path).unwrap();

    let analysis = analyze_rust_test_refs(&[&parsed_alpha, &parsed_beta, &parsed_test], None);

    let uncovered: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (d.name.as_str(), d.file.to_str().unwrap()))
        .collect();
    assert!(
        !analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "new" && d.file == alpha_path),
        "Foo::new should be covered (test calls Foo::new()), but unreferenced: {uncovered:?}"
    );
}

#[test]
fn test_insert_path_segments() {
    let path: syn::Path = syn::parse_str("foo::bar::Baz").unwrap();
    let mut refs = HashSet::new();
    references::insert_path_segments(&path, &mut refs);
    assert!(refs.contains("foo"));
    assert!(refs.contains("bar"));
    assert!(refs.contains("Baz"));
    let std_path: syn::Path = syn::parse_str("std::io::Read").unwrap();
    references::insert_path_segments(&std_path, &mut refs);
    assert!(!refs.contains("io"));
}

#[test]
fn test_collect_rust_references_for_fn_direct() {
    let code = "fn test_fn() { foo(); bar::baz(); }";
    let ast: syn::File = syn::parse_str(code).unwrap();
    if let syn::Item::Fn(f) = &ast.items[0] {
        let refs = references::collect_rust_references_for_fn(f);
        assert!(refs.contains("foo"));
    }
}

#[test]
fn test_collect_per_test_usage_from_items_direct() {
    let code = "#[cfg(test)] mod tests { #[test] fn test_it() { foo(); } }";
    let ast: syn::File = syn::parse_str(code).unwrap();
    let mut out = Vec::new();
    references::collect_per_test_usage_from_items(&ast.items, "", &mut out);
    assert!(!out.is_empty());
}

#[test]
fn test_visit_macro_tokens_direct() {
    let tokens: proc_macro2::TokenStream = "foo(bar)".parse().unwrap();
    let mut refs = HashSet::new();
    references::visit_macro_tokens(&tokens, &mut refs);
    assert!(refs.contains("foo") || refs.contains("bar"));
}

#[test]
fn test_is_binary_entry_point() {
    assert!(definitions::is_binary_entry_point(Path::new("src/main.rs")));
    assert!(definitions::is_binary_entry_point(Path::new("main.rs")));
    assert!(definitions::is_binary_entry_point(Path::new(
        "src/bin/foo.rs"
    )));
    assert!(definitions::is_binary_entry_point(Path::new(
        "legacy_tests/src/main.rs",
    )));
    assert!(!definitions::is_binary_entry_point(Path::new("src/lib.rs")));
    assert!(!definitions::is_binary_entry_point(Path::new(
        "tests/main.rs"
    )));
}

#[test]
fn test_trivial_binary_main_detection() {
    fn check(code: &str, path: &str, expect_trivial: bool, msg: &str) {
        let ast: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Fn(f) = &ast.items[0] {
            assert_eq!(
                definitions::is_trivial_binary_main(f, Path::new(path)),
                expect_trivial,
                "{msg}"
            );
        }
    }
    check(
        "fn main() { lib::run(); }",
        "src/main.rs",
        true,
        "qualified call",
    );
    check(
        "fn main() -> Result<(), E> { lib::run()?; Ok(()) }",
        "main.rs",
        true,
        "? operator",
    );
    check(
        "fn main() { if let Err(e) = lib::run() { std::process::exit(1); } }",
        "main.rs",
        true,
        "error handling",
    );
    check(
        "fn main() { run(); }",
        "src/main.rs",
        false,
        "unqualified call",
    );
    check("fn main() { fn h() {} h(); }", "main.rs", false, "local fn");
    check(
        "fn main() { lib::run(); }",
        "src/lib.rs",
        false,
        "not entry point",
    );
    // Macro bodies are not analyzed; a `main` that only contains macros is not necessarily
    // a thin delegate and should still count as a definition for test-reference coverage.
    check(
        "fn main() { println!(\"hello\"); }",
        "src/main.rs",
        false,
        "macro-only body",
    );
}

/// Qualified calls with non-trivial arguments must not count as thin delegation.
#[test]
fn test_trivial_binary_main_rejects_qualified_call_with_unvetted_arguments() {
    let ast: syn::File = syn::parse_str("fn main() { lib::run(compute()); }").unwrap();
    let syn::Item::Fn(f) = &ast.items[0] else {
        panic!("expected fn");
    };
    assert!(
        !definitions::is_trivial_binary_main(f, Path::new("src/main.rs")),
        "arguments to a qualified call must be analyzed; otherwise real work can hide under a qualified callee"
    );
}

/// Method calls must vet arguments, not only the receiver.
#[test]
fn test_trivial_binary_main_rejects_method_call_with_unvetted_arguments() {
    let ast: syn::File = syn::parse_str("fn main() { x.foo(bar()); }").unwrap();
    let syn::Item::Fn(f) = &ast.items[0] else {
        panic!("expected fn");
    };
    assert!(
        !definitions::is_trivial_binary_main(f, Path::new("src/main.rs")),
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
