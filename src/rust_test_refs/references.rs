use super::{has_cfg_test_attribute, has_test_attribute};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, Item};

pub(super) fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    ReferenceVisitor { refs }.visit_file(ast);
}

/// Collects references from a single function body. Returns the set of referenced names.
pub(crate) fn collect_rust_references_for_fn(f: &syn::ItemFn) -> HashSet<String> {
    let mut refs = HashSet::new();
    ReferenceVisitor { refs: &mut refs }.visit_item_fn(f);
    refs
}

/// Collects per-test (`test_id`, `usage_refs`) from a file.
/// `test_id` format: `fn_name` for top-level `#[test]` fn, `mod_name::fn_name` for `#[cfg(test)]` mod.
pub(super) fn collect_per_test_usage(ast: &syn::File) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_usage_from_items(&ast.items, "", &mut out);
    out
}

pub(crate) fn collect_per_test_usage_from_items(
    items: &[syn::Item],
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    for item in items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                let mod_name = m.ident.to_string();
                let mod_prefix = if prefix.is_empty() {
                    mod_name.clone()
                } else {
                    format!("{prefix}::{mod_name}")
                };
                if let Some((_, mod_items)) = &m.content {
                    collect_per_test_usage_from_items(mod_items, &mod_prefix, out);
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                let fn_name = f.sig.ident.to_string();
                let refs = collect_rust_references_for_fn(f);
                let test_id = if prefix.is_empty() {
                    fn_name
                } else {
                    format!("{prefix}::{fn_name}")
                };
                out.push((test_id, refs));
            }
            _ => {}
        }
    }
}

pub(super) struct ReferenceVisitor<'a> {
    pub(super) refs: &'a mut HashSet<String>,
}

pub(crate) fn is_external_crate(name: &str) -> bool {
    matches!(
        name,
        "std"
            | "core"
            | "alloc"
            | "syn"
            | "proc_macro"
            | "proc_macro2"
            | "quote"
            | "serde"
            | "serde_json"
            | "tokio"
            | "async_std"
            | "futures"
            | "anyhow"
            | "thiserror"
            | "clap"
            | "log"
            | "tracing"
            | "regex"
            | "chrono"
            | "uuid"
            | "rand"
            | "reqwest"
            | "hyper"
            | "axum"
            | "actix"
            | "diesel"
            | "sqlx"
            | "sea_orm"
            | "rocket"
            | "warp"
            | "tide"
            | "petgraph"
            | "tempfile"
            | "ignore"
            | "tree_sitter"
            | "tree_sitter_python"
            | "rayon"
            | "itertools"
    )
}

pub(crate) fn starts_with_external_crate(path: &syn::Path) -> bool {
    path.segments
        .first()
        .is_some_and(|s| is_external_crate(&s.ident.to_string()))
}

pub(crate) fn is_rust_keyword(name: &str) -> bool {
    matches!(name, "self" | "Self" | "super" | "crate")
}

pub(super) fn insert_path_segments(path: &syn::Path, refs: &mut HashSet<String>) {
    if starts_with_external_crate(path) {
        return;
    }
    for seg in &path.segments {
        let name = seg.ident.to_string();
        if !is_rust_keyword(&name) {
            refs.insert(name);
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    insert_path_segments(&p.path, self.refs);
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
            }
            Expr::Struct(s) => insert_path_segments(&s.path, self.refs),
            Expr::Path(p) => insert_path_segments(&p.path, self.refs),
            Expr::Macro(m) => visit_macro_tokens(&m.mac.tokens, self.refs),
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
    fn visit_type(&mut self, ty: &'ast syn::Type) {
        if let syn::Type::Path(p) = ty {
            insert_path_segments(&p.path, self.refs);
        }
        syn::visit::visit_type(self, ty);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        visit_macro_tokens(&mac.tokens, self.refs);
        syn::visit::visit_macro(self, mac);
    }
}

struct ExprList(Vec<Expr>);
impl syn::parse::Parse for ExprList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut exprs = Vec::new();
        while !input.is_empty() {
            exprs.push(input.parse()?);
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(Self(exprs))
    }
}

pub(super) fn try_parse_as_single_expr(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
) -> bool {
    if let Ok(e) = syn::parse2::<Expr>(tokens.clone()) {
        ReferenceVisitor { refs }.visit_expr(&e);
        return true;
    }
    false
}

pub(super) fn try_parse_as_expr_list(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
) -> bool {
    if let Ok(ExprList(exprs)) = syn::parse2::<ExprList>(tokens.clone()) {
        for e in exprs {
            ReferenceVisitor { refs }.visit_expr(&e);
        }
        return true;
    }
    false
}

pub(super) fn visit_nested_token_groups(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
) {
    for t in tokens.clone() {
        if let proc_macro2::TokenTree::Group(g) = t {
            visit_macro_tokens(&g.stream(), refs);
        }
    }
}

pub(crate) fn visit_macro_tokens(tokens: &proc_macro2::TokenStream, refs: &mut HashSet<String>) {
    if try_parse_as_single_expr(tokens, refs) {
        return;
    }
    if try_parse_as_expr_list(tokens, refs) {
        return;
    }
    visit_nested_token_groups(tokens, refs);
}
