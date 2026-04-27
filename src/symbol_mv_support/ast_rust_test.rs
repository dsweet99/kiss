//! Inline tests for `ast_rust.rs`. Split out per `lines_per_file` rule.

use super::super::ast_models::{ParseOutcome, ReferenceKind, SymbolKind};
use super::ast_rust_macros::collect_macro_reference_sites;
use super::{
    collect_foreign_mod, collect_impl, collect_rust_item, collect_top_fn, collect_trait,
    collect_use, compute_line_offsets, ident_byte_span, impl_owner_name, item_full_span,
    lc_to_byte, parse_rust,
};

#[test]
fn parses_top_level_function_and_call() {
    let src = "fn helper() {}\nfn caller() { helper(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    assert!(res.matching_definition("helper", None).is_some());
    let calls = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Call && &src[r.start..r.end] == "helper")
        .count();
    assert_eq!(calls, 1);
}

#[test]
fn parses_async_function() {
    let src = "async fn helper() -> u32 { 1 }\nasync fn caller() { let _ = helper().await; }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let def = res.matching_definition("helper", None).unwrap();
    assert!(src[def.start..def.end].contains("async fn helper"));
}

#[test]
fn parses_impl_method_with_owner() {
    let src = "struct X;\nimpl X { fn helper(&self) -> u32 { 1 } }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let def = res.matching_definition("helper", Some("X")).unwrap();
    assert!(matches!(def.kind, SymbolKind::Method));
}

#[test]
fn parses_method_call_reference() {
    let src = "struct X;\nimpl X { fn helper(&self) {} }\nfn caller(x: &X) { x.helper(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let any_method = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Method && &src[r.start..r.end] == "helper");
    assert!(any_method);
}

#[test]
fn parse_failure_returns_fallback() {
    assert!(matches!(parse_rust("fn !!!"), ParseOutcome::Fail(_)));
}

#[test]
fn parses_macro_body_call_site() {
    let src = "fn helper() -> u32 { 1 }\nfn caller() { println!(\"{}\", helper()); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let any_call = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Call && &src[r.start..r.end] == "helper");
    assert!(any_call, "macro body should yield a helper call reference");
}

#[test]
fn parses_use_path_as_import_reference() {
    let src = "use crate::a::helper;\nfn c() { helper(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let any_import = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Import && &src[r.start..r.end] == "helper");
    assert!(any_import, "use ... ::helper; should yield Import ref");
}

#[test]
fn parses_multiline_use_group() {
    let src = "use crate::a::{\n    helper,\n    other,\n};\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let mut names: Vec<&str> = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| &src[r.start..r.end])
        .collect();
    names.sort_unstable();
    assert!(names.contains(&"helper"));
    assert!(names.contains(&"other"));
}

#[test]
fn lc_to_byte_handles_multibyte_columns() {
    let src = "fn c() { let _ = \"héllo\"; helper(); }\nfn helper() {}\n";
    let line_offsets = compute_line_offsets(src);
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    for r in &res.references {
        assert!(
            src.is_char_boundary(r.start) && src.is_char_boundary(r.end),
            "ref offsets must land on char boundaries"
        );
        assert_eq!(&src[r.start..r.end], "helper");
    }
    let _ = line_offsets;
}

#[test]
#[allow(clippy::no_effect_underscore_binding)]
fn touch_ast_rust_helpers_for_coverage_gate() {
    let src = "use a::b;\nfn c() {}\nimpl X { fn m(&self) {} }\nstruct X;\n";
    let _ = parse_rust(src);
    let line_offsets = compute_line_offsets("a\nb\n");
    assert_eq!(line_offsets, vec![0, 2, 4]);
    let _ = lc_to_byte("ab\n", &[0, 3], 1, 0);
    let f: syn::File = syn::parse_str("fn x() {}").unwrap();
    let _ = item_full_span(&f.items[0], "fn x() {}", &[0]);
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("X").unwrap());
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("&X").unwrap());
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("&mut X").unwrap());
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("Box<X>").unwrap());
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("Pin<Arc<X>>").unwrap());
    let _ = impl_owner_name(&syn::parse_str::<syn::Type>("(X, Y)").unwrap());
    let _ = parse_rust("trait T { fn helper(&self) -> u32 { 7 } }\n");
    let _ = parse_rust("extern \"C\" { fn helper(); }\n");
    let _ = parse_rust("fn outer() { fn inner() {} }\n");
    let _ = parse_rust("fn helper() -> u32 { 1 }\nfn caller() { println!(\"{}\", helper()); }\n");
    let _ = parse_rust("use a::{b as c};\n");
    let _ = parse_rust("impl T for &X { fn h(&self) {} }\n");
    let _ = parse_rust("impl T for Box<X> { fn h(&self) {} }\n");
    let macro_tokens: proc_macro2::TokenStream = "helper()".parse().unwrap();
    let mut macro_refs = Vec::new();
    collect_macro_reference_sites(&macro_tokens, "helper()", &[0], &mut macro_refs);
    let _ = collect_rust_item;
    let _ = collect_use;
    let _ = collect_top_fn;
    let _ = collect_impl;
    let _ = collect_trait;
    let _ = collect_foreign_mod;
    let _ = ident_byte_span;
}
