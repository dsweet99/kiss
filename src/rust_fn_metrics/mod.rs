use syn::visit::Visit;
use syn::{Block, Expr, Pat, Stmt};

use crate::rust_parsing::ParsedRustFile;

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;

#[derive(Debug, Default)]
pub struct RustFunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub bool_parameters: usize,
    pub attributes: usize,
    pub calls: usize,
}

#[derive(Debug, Default)]
pub struct RustTypeMetrics {
    pub methods: usize,
}

#[derive(Debug, Default)]
pub struct RustFileMetrics {
    pub statements: usize,
    pub interface_types: usize,
    pub concrete_types: usize,
    pub imports: usize,
    pub functions: usize,
}

#[must_use]
pub fn compute_rust_file_metrics(parsed: &ParsedRustFile) -> RustFileMetrics {
    let mut metrics = RustFileMetrics::default();
    accumulate_rust_file_metrics_from_items(&parsed.ast.items, &mut metrics);
    metrics
}

fn accumulate_rust_file_metrics_from_items(items: &[syn::Item], out: &mut RustFileMetrics) {
    for item in items {
        match item {
            syn::Item::Trait(_) => out.interface_types += 1,
            syn::Item::Struct(_) | syn::Item::Enum(_) | syn::Item::Union(_) => {
                out.concrete_types += 1;
            }
            syn::Item::Use(u) if matches!(u.vis, syn::Visibility::Inherited) => {
                out.imports += count_use_names(&u.tree);
            }
            syn::Item::Fn(f) => {
                out.functions += 1;
                let mut visitor = FunctionMetricsVisitor::default();
                visitor.visit_block(&f.block);
                out.statements += visitor.statements;
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(f) = impl_item {
                        out.functions += 1;
                        let mut visitor = FunctionMetricsVisitor::default();
                        visitor.visit_block(&f.block);
                        out.statements += visitor.statements;
                    }
                }
            }
            syn::Item::Mod(m) => {
                if !is_cfg_test_mod(m)
                    && let Some((_, nested_items)) = &m.content
                {
                    accumulate_rust_file_metrics_from_items(nested_items, out);
                }
            }
            _ => {}
        }
    }
}

/// Returns true if the module has a `#[cfg(test)]` or similar attribute indicating test code.
///
/// Handles compound expressions like `#[cfg(all(test, ...))]` and negations
/// like `#[cfg(not(test))]` (returns false) and `#[cfg(not(not(test)))]` (returns true).
pub fn is_cfg_test_mod(m: &syn::ItemMod) -> bool {
    m.attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<proc_macro2::TokenStream>()
            .map(|ts| contains_test_ident(ts, false))
            .unwrap_or(false)
    })
}

fn contains_test_ident(tokens: proc_macro2::TokenStream, negated: bool) -> bool {
    use proc_macro2::TokenTree;
    let mut iter = tokens.into_iter();
    while let Some(tt) = iter.next() {
        match &tt {
            TokenTree::Ident(id) if *id == "test" => {
                // If we're inside an odd number of not() wrappers, this is NOT test code
                if !negated {
                    return true;
                }
            }
            TokenTree::Ident(id) if *id == "not" => {
                // Recurse into not() with flipped negation
                if let Some(TokenTree::Group(g)) = iter.next()
                    && contains_test_ident(g.stream(), !negated)
                {
                    return true;
                }
            }
            TokenTree::Ident(id) if *id == "all" || *id == "any" => {
                // Recurse into all/any groups, preserving current negation
                if let Some(TokenTree::Group(g)) = iter.next()
                    && contains_test_ident(g.stream(), negated)
                {
                    return true;
                }
            }
            TokenTree::Group(g) => {
                if contains_test_ident(g.stream(), negated) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Count attributes excluding doc comments (`#[doc = "..."]` / `///` lowered to `doc`).
///
/// Matches `kiss check` / `annotations_per_function` so stats and detailed output use the same rule.
#[must_use]
pub fn count_non_doc_attrs(attrs: &[syn::Attribute]) -> usize {
    attrs.iter().filter(|a| !a.path().is_ident("doc")).count()
}

/// Count the number of individual names imported by a `use` tree.
/// `use foo::bar;` → 1, `use foo::{bar, baz};` → 2, `use foo::*;` → 1 (glob counts as 1).
fn count_use_names(tree: &syn::UseTree) -> usize {
    match tree {
        syn::UseTree::Path(p) => count_use_names(&p.tree),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => 1,
        syn::UseTree::Group(g) => g.items.iter().map(count_use_names).sum(),
    }
}

// Allow: metrics are computed incrementally from different sources (args, visitor, attr_count)
#[allow(clippy::field_reassign_with_default)]
pub fn compute_rust_function_metrics(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
    attr_count: usize,
) -> RustFunctionMetrics {
    let mut metrics = RustFunctionMetrics::default();

    let non_self_args: Vec<_> = inputs
        .iter()
        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
        .collect();
    metrics.arguments = non_self_args.len();
    metrics.bool_parameters = non_self_args
        .iter()
        .filter(|arg| is_bool_param(arg))
        .count();
    metrics.attributes = attr_count;

    let mut visitor = FunctionMetricsVisitor::default();
    visitor.visit_block(block);

    metrics.statements = visitor.statements;
    metrics.max_indentation = visitor.max_depth;
    metrics.returns = visitor.returns;
    metrics.branches = visitor.branches;
    metrics.local_variables = visitor.local_variables;
    metrics.nested_function_depth = visitor.max_closure_depth;
    metrics.calls = visitor.calls;

    metrics
}

pub(crate) fn is_bool_param(arg: &syn::FnArg) -> bool {
    matches!(arg, syn::FnArg::Typed(pt) if matches!(&*pt.ty, syn::Type::Path(tp) if tp.path.is_ident("bool")))
}

#[derive(Default)]
pub struct FunctionMetricsVisitor {
    pub statements: usize,
    pub max_depth: usize,
    pub current_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub max_closure_depth: usize,
    pub current_closure_depth: usize,
    pub calls: usize,
}

impl FunctionMetricsVisitor {
    pub fn enter_block(&mut self) {
        self.current_depth += 1;
        self.max_depth = self.max_depth.max(self.current_depth);
    }

    pub const fn exit_block(&mut self) {
        self.current_depth -= 1;
    }

    pub fn count_pattern_bindings(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(_) => self.local_variables += 1,
            Pat::Type(typed) => self.count_pattern_bindings(&typed.pat),
            Pat::Tuple(tuple) => {
                for elem in &tuple.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::TupleStruct(ts) => {
                for elem in &ts.elems {
                    self.count_pattern_bindings(elem);
                }
            }
            Pat::Struct(s) => {
                for field in &s.fields {
                    self.count_pattern_bindings(&field.pat);
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for FunctionMetricsVisitor {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        // Statement definition: statements exclude imports and signatures.
        // Skip use statements inside function bodies
        let is_use_item = matches!(stmt, Stmt::Item(syn::Item::Use(_)));
        // Skip inner fn items: they are separate scopes whose body metrics
        // should not be attributed to the enclosing function.
        let is_inner_fn = matches!(stmt, Stmt::Item(syn::Item::Fn(_)));
        if !is_use_item {
            self.statements += 1;
        }
        if let Stmt::Local(local) = stmt {
            self.count_pattern_bindings(&local.pat);
        }
        if !is_inner_fn {
            syn::visit::visit_stmt(self, stmt);
        }
    }

    // Note: Rust match arms are NOT counted as branches (unlike Python case clauses).
    // Rust match is exhaustive pattern matching; Python match/case is optional branching.
    // This preserves semantic consistency: we count optional code paths, not exhaustive coverage.

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::If(_) => {
                self.branches += 1;
                self.enter_block();
            }
            Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.enter_block();
            }
            Expr::Return(_) => {
                self.returns += 1;
            }
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
            }
            Expr::Call(_) | Expr::MethodCall(_) => {
                self.calls += 1;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
        match expr {
            Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.exit_block();
            }
            Expr::Closure(_) => self.current_closure_depth -= 1,
            _ => {}
        }
    }
}
