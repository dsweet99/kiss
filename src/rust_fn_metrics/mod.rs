use std::path::Path;

use syn::visit::Visit;
use syn::{Block, Expr, Pat, Stmt};

use crate::code_roles::{SourceRoleIndex, skip_syn};
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
    compute_rust_file_metrics_with_roles(parsed, None)
}

#[must_use]
pub fn compute_rust_file_metrics_with_roles(
    parsed: &ParsedRustFile,
    roles: Option<&SourceRoleIndex>,
) -> RustFileMetrics {
    let mut metrics = RustFileMetrics::default();
    accumulate_rust_file_metrics_from_items(&parsed.path, &parsed.ast.items, &mut metrics, roles);
    metrics
}

fn accumulate_rust_file_metrics_from_items(
    path: &Path,
    items: &[syn::Item],
    out: &mut RustFileMetrics,
    roles: Option<&SourceRoleIndex>,
) {
    for item in items {
        if skip_syn(roles, path, item) {
            continue;
        }
        accumulate_item(path, item, out, roles);
    }
}

fn accumulate_item(
    path: &Path,
    item: &syn::Item,
    out: &mut RustFileMetrics,
    roles: Option<&SourceRoleIndex>,
) {
    match item {
        syn::Item::Trait(_) => out.interface_types += 1,
        syn::Item::Struct(_) | syn::Item::Enum(_) | syn::Item::Union(_) => add_concrete_type(out),
        syn::Item::Use(u) => {
            if matches!(u.vis, syn::Visibility::Inherited) {
                add_import_names(out, &u.tree);
            }
        }
        syn::Item::Fn(f) => {
            out.functions += 1;
            out.statements += stmt_count(path, &f.block, roles);
        }
        syn::Item::Impl(imp) => add_impl_fn_metrics(path, imp, out, roles),
        syn::Item::Mod(m) => add_mod_metrics(path, m, out, roles),
        _ => {}
    }
}

fn add_impl_fn_metrics(
    path: &Path,
    imp: &syn::ItemImpl,
    out: &mut RustFileMetrics,
    roles: Option<&SourceRoleIndex>,
) {
    for impl_item in &imp.items {
        let syn::ImplItem::Fn(func) = impl_item else {
            continue;
        };
        if skip_syn(roles, path, func) {
            continue;
        }
        out.functions += 1;
        out.statements += stmt_count(path, &func.block, roles);
    }
}

fn add_mod_metrics(
    path: &Path,
    module: &syn::ItemMod,
    out: &mut RustFileMetrics,
    roles: Option<&SourceRoleIndex>,
) {
    if let Some((_, nested_items)) = &module.content {
        accumulate_rust_file_metrics_from_items(path, nested_items, out, roles);
    }
}

fn stmt_count(path: &Path, block: &Block, roles: Option<&SourceRoleIndex>) -> usize {
    let mut visitor = FunctionMetricsVisitor {
        path: Some(path),
        roles,
        ..FunctionMetricsVisitor::default()
    };
    visitor.visit_block(block);
    visitor.statements
}

fn add_concrete_type(out: &mut RustFileMetrics) {
    out.concrete_types += 1;
}

fn add_import_names(out: &mut RustFileMetrics, tree: &syn::UseTree) {
    out.imports += count_use_names(tree);
}

#[must_use]
pub fn count_non_doc_attrs(attrs: &[syn::Attribute]) -> usize {
    attrs.iter().filter(|a| !a.path().is_ident("doc")).count()
}

fn count_use_names(tree: &syn::UseTree) -> usize {
    match tree {
        syn::UseTree::Path(p) => count_use_names(&p.tree),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => 1,
        syn::UseTree::Group(g) => g.items.iter().map(count_use_names).sum(),
    }
}

#[allow(clippy::field_reassign_with_default)]
pub fn compute_rust_function_metrics(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
    attr_count: usize,
) -> RustFunctionMetrics {
    compute_rust_function_metrics_with_roles(inputs, block, attr_count, None, None)
}

pub fn compute_rust_function_metrics_with_roles(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &Block,
    attr_count: usize,
    path: Option<&Path>,
    roles: Option<&SourceRoleIndex>,
) -> RustFunctionMetrics {
    let mut metrics = RustFunctionMetrics::default();

    let non_self_args: Vec<_> = inputs
        .iter()
        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
        .filter(|arg| match (path, roles, arg) {
            (Some(path), Some(roles), syn::FnArg::Typed(pat)) => !skip_syn(Some(roles), path, pat),
            _ => true,
        })
        .collect();
    metrics.arguments = non_self_args.len();
    metrics.bool_parameters = non_self_args
        .iter()
        .filter(|arg| is_bool_param(arg))
        .count();
    metrics.attributes = attr_count;

    let mut visitor = FunctionMetricsVisitor {
        path,
        roles,
        ..FunctionMetricsVisitor::default()
    };
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
pub struct FunctionMetricsVisitor<'a> {
    pub statements: usize,
    pub max_depth: usize,
    pub current_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub max_closure_depth: usize,
    pub current_closure_depth: usize,
    pub calls: usize,
    path: Option<&'a Path>,
    roles: Option<&'a SourceRoleIndex>,
}

impl FunctionMetricsVisitor<'_> {
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

impl<'ast> Visit<'ast> for FunctionMetricsVisitor<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let (Some(path), Some(roles)) = (self.path, self.roles)
            && skip_syn(Some(roles), path, stmt)
        {
            return;
        }
        let is_use_item = matches!(stmt, Stmt::Item(syn::Item::Use(_)));

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

    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.on_enter_expr(expr);
        syn::visit::visit_expr(self, expr);
        self.on_exit_expr(expr);
    }
}

impl FunctionMetricsVisitor<'_> {
    fn on_enter_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::If(_) => {
                self.branches += 1;
                self.enter_block();
            }
            Expr::Match(m) => {
                let arms = m.arms.iter().filter(|arm| match (self.path, self.roles) {
                    (Some(path), Some(roles)) => !skip_syn(Some(roles), path, *arm),
                    _ => true,
                });
                self.branches += arms.count();
                self.enter_block();
            }
            Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => self.enter_block(),
            Expr::Return(_) => self.returns += 1,
            Expr::Closure(_) => {
                self.current_closure_depth += 1;
                self.max_closure_depth = self.max_closure_depth.max(self.current_closure_depth);
            }
            Expr::Call(_) | Expr::MethodCall(_) => self.calls += 1,
            _ => {}
        }
    }

    fn on_exit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                self.exit_block();
            }
            Expr::Closure(_) => self.current_closure_depth -= 1,
            _ => {}
        }
    }
}
