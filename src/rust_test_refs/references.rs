use super::{has_cfg_test_attribute, has_test_attribute};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Attribute, Expr, Item};

#[path = "references_macro.rs"]
mod references_macro;
pub(crate) use references_macro::visit_macro_tokens;
#[cfg(test)]
pub(crate) use references_macro::{
    try_parse_as_expr_list, try_parse_as_single_expr, visit_nested_token_groups,
};

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

pub(crate) fn collect_rust_references(ast: &syn::File, refs: &mut HashSet<String>) {
    collect_rust_references_with_mode(ast, refs, RefWitnessMode::GATE);
}

/// Collect references for coverage-map calibration: calls, method calls, struct literals,
/// macro innards, and function-item call arguments (not bare paths, types, or path strings).
pub(crate) fn collect_rust_references_for_coverage_map(ast: &syn::File, refs: &mut HashSet<String>) {
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
    collect_test_parametric_attr_macro_witnesses(&f.attrs, &mut refs, RefWitnessMode::COVERAGE_MAP);
    refs
}

/// `#[test_case(Rule::Foo, …)]` / `#[rstest(…)]` put witnesses in attribute tokens; `visit_attribute`
/// does not descend into them, so collect them explicitly for coverage calibration.
pub(super) fn is_test_parametric_attribute(attr: &Attribute) -> bool {
    attr.path().is_ident("test_case")
        || attr.path().is_ident("rstest")
        || attr.path().is_ident("case")
        || attr
            .path()
            .segments
            .last()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "test_case" | "rstest" | "case"))
}

pub(super) fn has_test_parametric_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(is_test_parametric_attribute)
}

pub(super) fn collect_test_parametric_attr_macro_witnesses(
    attrs: &[Attribute],
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    for attr in attrs {
        if !is_test_parametric_attribute(attr) {
            continue;
        }
        if let syn::Meta::List(list) = &attr.meta {
            visit_macro_tokens(&list.tokens, refs, mode);
        }
    }
}

pub(super) fn collect_fn_test_attr_macro_witnesses(
    items: &[Item],
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, sub)) = &m.content {
                    collect_fn_test_attr_macro_witnesses(sub, refs, mode);
                }
            }
            Item::Fn(f) => {
                collect_test_parametric_attr_macro_witnesses(&f.attrs, refs, mode);
            }
            _ => {}
        }
    }
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
            Item::Fn(f)
                if has_test_attribute(&f.attrs) || has_test_parametric_attribute(&f.attrs) =>
            {
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

/// `Rule::ShebangNotExecutable` in `test_case` witnesses the variant ident, not `shebang_not_executable`.
pub(crate) fn insert_rule_variant_snake_alias(path: &syn::Path, refs: &mut HashSet<String>) {
    if path.segments.len() < 2 {
        return;
    }
    let first = path.segments.first().map(|s| s.ident.to_string());
    if first.as_deref() != Some("Rule") {
        return;
    }
    let variant = path.segments.last().map(|s| s.ident.to_string());
    let Some(variant) = variant else {
        return;
    };
    if let Some(snake) = pascal_case_ident_to_snake(&variant) {
        refs.insert(snake);
    }
}

fn pascal_case_ident_to_snake(name: &str) -> Option<String> {
    if name.is_empty() || !name.chars().skip(1).any(char::is_uppercase) {
        return None;
    }
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.char_indices() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    Some(out)
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
    insert_rule_variant_snake_alias(path, refs);
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
            Expr::Path(p) if self.mode.bare_paths || p.path.segments.len() >= 2 => {
                insert_path_segments(&p.path, self.refs);
            }
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
