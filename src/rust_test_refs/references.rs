use super::{has_cfg_test_attribute, has_test_attribute};
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, Item};

#[derive(Clone, Copy)]
pub(super) struct RefWitnessMode {
    pub(super) bare_paths: bool,
    pub(super) path_string_literals: bool,
}

impl RefWitnessMode {
    pub(super) const GATE: Self = Self {
        bare_paths: true,
        path_string_literals: true,
    };
    pub(super) const COVERAGE_MAP: Self = Self {
        bare_paths: false,
        path_string_literals: false,
    };

    pub(super) const fn includes_bare_paths(self) -> bool {
        self.bare_paths
    }
}

pub(super) fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    collect_rust_references_with_mode(ast, refs, RefWitnessMode::GATE);
}

/// Collect references for coverage-map calibration: calls, method calls, struct literals,
/// macro innards, and function-item call arguments (not bare paths, types, or path strings).
pub(super) fn collect_rust_references_for_coverage_map(ast: &syn::File, refs: &mut HashSet<String>) {
    collect_rust_references_with_mode(ast, refs, RefWitnessMode::COVERAGE_MAP);
}

fn collect_rust_references_with_mode(
    ast: &syn::File,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    ReferenceVisitor { refs, mode }.visit_file(ast);
}

/// Collects references from a single function body. Returns the set of referenced names.
pub(crate) fn collect_rust_references_for_fn(f: &syn::ItemFn) -> HashSet<String> {
    let mut refs = HashSet::new();
    ReferenceVisitor {
        refs: &mut refs,
        mode: RefWitnessMode::GATE,
    }
    .visit_item_fn(f);
    refs
}

pub(crate) fn collect_rust_references_for_fn_coverage_map(f: &syn::ItemFn) -> HashSet<String> {
    let mut refs = HashSet::new();
    ReferenceVisitor {
        refs: &mut refs,
        mode: RefWitnessMode::COVERAGE_MAP,
    }
    .visit_item_fn(f);
    refs
}

/// Collects per-test (`test_id`, `usage_refs`) from a file.
/// `test_id` format: `fn_name` for top-level `#[test]` fn, `mod_name::fn_name` for `#[cfg(test)]` mod.
pub(super) fn collect_per_test_usage(ast: &syn::File) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_usage_from_items(&ast.items, "", &mut out, RefWitnessMode::GATE);
    out
}

pub(super) fn collect_per_test_usage_for_coverage_map(
    ast: &syn::File,
) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_usage_from_items(&ast.items, "", &mut out, RefWitnessMode::COVERAGE_MAP);
    out
}

#[must_use]
pub fn rust_test_functions_in(parsed: &crate::rust_parsing::ParsedRustFile) -> Vec<String> {
    collect_per_test_usage(&parsed.ast)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

pub(crate) fn collect_per_test_usage_from_items(
    items: &[syn::Item],
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
    mode: RefWitnessMode,
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
                    collect_per_test_usage_from_items(mod_items, &mod_prefix, out, mode);
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                let fn_name = f.sig.ident.to_string();
                let refs = if mode.includes_bare_paths() {
                    collect_rust_references_for_fn(f)
                } else {
                    collect_rust_references_for_fn_coverage_map(f)
                };
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
    pub(super) mode: RefWitnessMode,
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

#[must_use]
pub(crate) fn is_stringify_macro(mac: &syn::Macro) -> bool {
    mac.path.is_ident("stringify")
}

pub(crate) fn insert_coverage_path_string_ref(value: &str, refs: &mut HashSet<String>) {
    if !value.contains(".rs::") {
        return;
    }
    let Some((_path, sym)) = value.rsplit_once("::") else {
        return;
    };
    let sym = sym.trim();
    let Some(first) = sym.chars().next() else {
        return;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return;
    }
    if sym
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        refs.insert(sym.to_string());
    }
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
                for arg in &c.args {
                    if let Expr::Path(p) = arg {
                        insert_path_segments(&p.path, self.refs);
                    }
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
            }
            Expr::Struct(s) => insert_path_segments(&s.path, self.refs),
            Expr::Path(p) if self.mode.bare_paths => insert_path_segments(&p.path, self.refs),
            Expr::Macro(m) if !is_stringify_macro(&m.mac) => {
                visit_macro_tokens(&m.mac.tokens, self.refs, self.mode);
            }
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) if self.mode.path_string_literals => {
                insert_coverage_path_string_ref(&s.value(), self.refs);
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
    fn visit_type(&mut self, ty: &'ast syn::Type) {
        if self.mode.bare_paths && let syn::Type::Path(p) = ty {
            insert_path_segments(&p.path, self.refs);
        }
        syn::visit::visit_type(self, ty);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if !is_stringify_macro(mac) {
            visit_macro_tokens(&mac.tokens, self.refs, self.mode);
        }
        syn::visit::visit_macro(self, mac);
    }
}

pub(super) fn try_parse_as_single_expr(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) -> bool {
    if let Some(e) = parse_single_expr(tokens) {
        ReferenceVisitor { refs, mode }.visit_expr(&e);
        return true;
    }
    false
}

pub(super) fn try_parse_as_expr_list(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) -> bool {
    if let Some(exprs) = parse_expr_list(tokens) {
        for e in exprs {
            ReferenceVisitor { refs, mode }.visit_expr(&e);
        }
        return true;
    }
    false
}

pub(super) fn visit_nested_token_groups(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    for t in tokens.clone() {
        if let proc_macro2::TokenTree::Group(g) = t {
            visit_macro_tokens(&g.stream(), refs, mode);
        }
    }
}

pub(crate) fn visit_macro_tokens(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    if try_parse_as_single_expr(tokens, refs, mode) {
        return;
    }
    if try_parse_as_expr_list(tokens, refs, mode) {
        return;
    }
    visit_nested_token_groups(tokens, refs, mode);
}
