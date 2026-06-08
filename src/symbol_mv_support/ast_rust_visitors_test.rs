use super::super::ast_models::{Reference, ReferenceKind};
use super::super::ast_rust_span::compute_line_offsets;
use super::{CallVisitor, NestedDefVisitor, collect_impl, impl_owner_name};
use syn::visit::Visit;

fn walk_source(src: &str) -> Vec<Reference> {
    let file = syn::parse_file(src).expect("valid rust fixture");
    let line_offsets = compute_line_offsets(src);
    let mut refs = Vec::new();
    let mut visitor = CallVisitor {
        content: src,
        line_offsets: &line_offsets,
        refs: &mut refs,
        in_call: false,
    };
    visitor.visit_file(&file);
    refs
}

#[test]
fn call_visitor_records_calls_methods_and_imports() {
    let src = r"
use crate::a::{helper as renamed};

macro_rules! m { ($x:expr) => { $x }; }

fn caller(x: &X) -> u32 {
    renamed();
    x.helper();
    let f = helper;
    let _ = f();
    m!(helper());
    helper
}

struct X;
impl X {
    fn helper(&self) -> u32 { 1 }
}
";
    let refs = walk_source(src);
    assert!(refs.iter().any(|r| r.kind == ReferenceKind::Import));
    assert!(refs.iter().any(|r| r.kind == ReferenceKind::Method));
    assert!(refs.iter().any(|r| r.kind == ReferenceKind::Call));
}

#[test]
fn expr_path_method_and_macro_visitor_hooks() {
    let src = "struct X;\nimpl X { fn helper(&self) -> u32 { 1 } }\nfn caller(x: &X) { x.helper(); let f = helper; }\nfn helper() -> u32 { 1 }\n";
    let file = syn::parse_file(src).unwrap();
    let line_offsets = compute_line_offsets(src);
    let mut refs = Vec::new();
    let mut visitor = CallVisitor {
        content: src,
        line_offsets: &line_offsets,
        refs: &mut refs,
        in_call: false,
    };
    visitor.visit_file(&file);

    let call: syn::ExprCall = syn::parse_str("helper()").unwrap();
    visitor.visit_expr_call(&call);
    let path: syn::ExprPath = syn::parse_str("helper").unwrap();
    visitor.visit_expr_path(&path);
    let method: syn::ExprMethodCall = syn::parse_str("x.helper()").unwrap();
    visitor.visit_expr_method_call(&method);
    let expr_macro: syn::ExprMacro = syn::parse_str("m!(helper())").unwrap();
    visitor.visit_expr_macro(&expr_macro);
    let stmt: syn::Stmt = syn::parse_str("m!(helper());").unwrap();
    if let syn::Stmt::Macro(stmt_macro) = stmt {
        visitor.visit_stmt_macro(&stmt_macro);
    }

    assert!(refs.iter().any(|r| r.kind == ReferenceKind::Method));
    assert!(refs.iter().any(|r| r.kind == ReferenceKind::Call));
}

#[test]
fn call_visitor_handles_stmt_macro() {
    let src = r"
macro_rules! m { ($x:expr) => { $x }; }
fn helper() -> u32 { 1 }
fn caller() { m!(helper()); }
";
    let refs = walk_source(src);
    assert!(refs.iter().any(|r| &src[r.start..r.end] == "helper"));
}

#[test]
fn use_path_name_and_rename_visitor_hooks() {
    let src = "use crate::path::item;\nuse group::{one, two as alias};\n";
    let file = syn::parse_file(src).unwrap();
    let line_offsets = compute_line_offsets(src);
    let mut refs = Vec::new();
    let mut visitor = CallVisitor {
        content: src,
        line_offsets: &line_offsets,
        refs: &mut refs,
        in_call: false,
    };
    visitor.visit_file(&file);

    let use_path = syn::parse_str::<syn::ItemUse>("use crate::path::item;").unwrap();
    if let syn::UseTree::Path(path) = use_path.tree {
        visitor.visit_use_path(&path);
    }
    let use_group = syn::parse_str::<syn::ItemUse>("use group::{one, two as alias};").unwrap();
    if let syn::UseTree::Group(group) = use_group.tree {
        for tree in group.items {
            match tree {
                syn::UseTree::Name(name) => visitor.visit_use_name(&name),
                syn::UseTree::Rename(rename) => visitor.visit_use_rename(&rename),
                _ => {}
            }
        }
    }

    let names: Vec<&str> = refs
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| &src[r.start..r.end])
        .collect();
    assert!(names.iter().any(|n| *n == "item" || *n == "path"));
    assert!(names.iter().any(|n| *n == "one" || *n == "alias"));
}

#[test]
fn collect_impl_and_nested_visitor_register_methods() {
    let src = "struct X;\nimpl X { fn helper(&self) -> u32 { 1 } }\nfn outer() { fn inner() {} }\n";
    let file = syn::parse_file(src).unwrap();
    let line_offsets = compute_line_offsets(src);
    let mut defs = Vec::new();
    let mut refs = Vec::new();
    for item in &file.items {
        if let syn::Item::Impl(item_impl) = item {
            collect_impl(item_impl, src, &line_offsets, &mut defs, &mut refs);
        }
    }
    let mut nested = NestedDefVisitor {
        content: src,
        line_offsets: &line_offsets,
        defs: &mut defs,
        depth: 0,
    };
    nested.visit_file(&file);
    assert!(defs.iter().any(|d| d.name == "helper"));
    assert!(impl_owner_name(&syn::parse_str("Box<X>").unwrap()).is_some());
}
