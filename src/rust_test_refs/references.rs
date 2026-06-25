use super::{has_cfg_test_attribute, has_test_attribute};
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, Item};

pub(crate) type QualifiedModuleRef = (String, String);

pub(super) fn collect_rust_references(
    ast: &syn::File,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    ReferenceVisitor { refs, qualified }.visit_file(ast);
}

/// Collects references from a single function body. Returns the set of referenced names.
pub(crate) fn collect_rust_references_for_fn(f: &syn::ItemFn) -> HashSet<String> {
    let mut refs = HashSet::new();
    let mut qualified = HashSet::new();
    ReferenceVisitor {
        refs: &mut refs,
        qualified: &mut qualified,
    }
    .visit_item_fn(f);
    refs
}

/// Collects per-test (`test_id`, `usage_refs`) from a file.
/// `test_id` format: `fn_name` for top-level `#[test]` fn, `mod_name::fn_name` for `#[cfg(test)]` mod.
pub(super) fn collect_per_test_usage(ast: &syn::File) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_usage_from_items(&ast.items, "", &mut out);
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

pub(super) fn insert_qualified_path_reference(
    path: &syn::Path,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    if starts_with_external_crate(path) {
        return;
    }
    insert_path_segments(path, refs);
    if path.segments.len() >= 2 {
        let name = path.segments.last().unwrap().ident.to_string();
        if is_rust_keyword(&name) {
            return;
        }
        let module = path
            .segments
            .iter()
            .take(path.segments.len() - 1)
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join(".");
        qualified.insert((module, name));
    }
}

pub(super) struct ReferenceVisitor<'a> {
    pub(super) refs: &'a mut HashSet<String>,
    pub(super) qualified: &'a mut HashSet<QualifiedModuleRef>,
}

pub(super) struct CallReferenceVisitor<'a> {
    pub(super) refs: &'a mut HashSet<String>,
    pub(super) qualified: &'a mut HashSet<QualifiedModuleRef>,
}

fn visit_item_skip_use<'a, V: Visit<'a>>(visitor: &mut V, item: &'a syn::Item) {
    if matches!(item, Item::Use(_)) {
        return;
    }
    syn::visit::visit_item(visitor, item);
}

impl<'ast> Visit<'ast> for CallReferenceVisitor<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        visit_item_skip_use(self, item);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    insert_qualified_path_reference(&p.path, self.refs, self.qualified);
                }
                for arg in &c.args {
                    self.visit_expr(arg);
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
                self.visit_expr(&m.receiver);
                for arg in &m.args {
                    self.visit_expr(arg);
                }
            }
            _ => syn::visit::visit_expr(self, expr),
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        visit_item_skip_use(self, item);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    insert_qualified_path_reference(&p.path, self.refs, self.qualified);
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
            }
            Expr::Struct(s) => {
                insert_qualified_path_reference(&s.path, self.refs, self.qualified);
            }
            Expr::Path(p) => insert_qualified_path_reference(&p.path, self.refs, self.qualified),
            Expr::Macro(m) => visit_macro_tokens(&m.mac.tokens, self.refs, self.qualified),
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
    fn visit_type(&mut self, ty: &'ast syn::Type) {
        // Type-position names (e.g. `size_of::<T>()`, `let _: T`) are not execution
        // witnesses; skip them so impl methods are not marked covered without value use.
        syn::visit::visit_type(self, ty);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        visit_macro_tokens(&mac.tokens, self.refs, self.qualified);
        syn::visit::visit_macro(self, mac);
    }
}

pub(super) fn try_parse_as_single_expr(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) -> bool {
    if let Some(e) = parse_single_expr(tokens) {
        ReferenceVisitor { refs, qualified }.visit_expr(&e);
        return true;
    }
    false
}

pub(super) fn try_parse_as_expr_list(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) -> bool {
    if let Some(exprs) = parse_expr_list(tokens) {
        for e in exprs {
            ReferenceVisitor { refs, qualified }.visit_expr(&e);
        }
        return true;
    }
    false
}

pub(super) fn visit_nested_token_groups(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    for t in tokens.clone() {
        if let proc_macro2::TokenTree::Group(g) = t {
            visit_macro_tokens(&g.stream(), refs, qualified);
        }
    }
}

pub(crate) fn visit_macro_tokens(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    if try_parse_as_single_expr(tokens, refs, qualified) {
        return;
    }
    if try_parse_as_expr_list(tokens, refs, qualified) {
        return;
    }
    visit_nested_token_groups(tokens, refs, qualified);
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::collections::HashSet;
    use syn::visit::Visit;

    impl ReferenceVisitor<'_> {
        fn coverage_witness_rv<'a>(
            refs: &'a mut HashSet<String>,
            qualified: &'a mut HashSet<QualifiedModuleRef>,
        ) -> ReferenceVisitor<'a> {
            ReferenceVisitor { refs, qualified }
        }
    }

    impl CallReferenceVisitor<'_> {
        fn coverage_witness_cv<'a>(
            refs: &'a mut HashSet<String>,
            qualified: &'a mut HashSet<QualifiedModuleRef>,
        ) -> CallReferenceVisitor<'a> {
            CallReferenceVisitor { refs, qualified }
        }
    }

    #[test]
    fn witness_reference_visitors() {
        let mut refs = HashSet::new();
        let mut qualified = HashSet::new();
        let mut rv = ReferenceVisitor::coverage_witness_rv(&mut refs, &mut qualified);
        let item: syn::Item = syn::parse_quote!(
            fn sample() {
                sample();
            }
        );
        rv.visit_item(&item);
        {
            let mut cv = CallReferenceVisitor::coverage_witness_cv(&mut refs, &mut qualified);
            if let syn::Item::Fn(f) = &item
                && let syn::Stmt::Expr(expr, _) = &f.block.stmts[0]
            {
                cv.visit_expr(expr);
            }
        }
        assert!(refs.contains("sample"));
    }
}

#[cfg(test)]
#[test]
fn witness_visit_item_skip_use_fn() {
    let mut refs = std::collections::HashSet::new();
    let mut qualified = std::collections::HashSet::new();
    let mut rv = ReferenceVisitor {
        refs: &mut refs,
        qualified: &mut qualified,
    };
    let item: syn::Item = syn::parse_quote!(
        use std;
    );
    visit_item_skip_use(&mut rv, &item);
}
