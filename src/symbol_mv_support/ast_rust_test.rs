//! Inline tests for `ast_rust.rs`. Split out per `lines_per_file` rule.

use super::super::ast_models::{ParseOutcome, ReferenceKind, SymbolKind};
use super::super::ast_rust_span::compute_line_offsets;
use super::parse_rust;

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
fn parses_braced_use_rename_as_import_reference() {
    let src = "use crate::a::{helper as renamed, plain};\nfn c() { renamed(); plain(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let imports: Vec<&str> = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| &src[r.start..r.end])
        .collect();
    assert!(imports.contains(&"helper"));
    assert!(imports.contains(&"plain"));
}

#[test]
fn parses_use_rename_as_import_reference() {
    let src = "use crate::a::{helper as renamed};\nfn c() { renamed(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let any_import = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Import && &src[r.start..r.end] == "helper");
    assert!(any_import);
}

#[test]
fn parses_function_as_value_reference() {
    let src = "fn helper() -> u32 { 1 }\nfn caller() { let f = helper; let _ = f(); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    let value_refs = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Call && &src[r.start..r.end] == "helper")
        .count();
    assert!(value_refs >= 1);
}

#[test]
fn parses_stmt_macro_reference() {
    let src = "macro_rules! m { ($x:expr) => { $x }; }\nfn helper() -> u32 { 1 }\nfn caller() { m!(helper()); }\n";
    let ParseOutcome::Success(res) = parse_rust(src) else {
        panic!("parse should succeed");
    };
    assert!(
        res.references
            .iter()
            .any(|r| &src[r.start..r.end] == "helper")
    );
}
