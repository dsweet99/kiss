use super::references::{QualifiedModuleRef, insert_qualified_path_reference};
use super::{has_cfg_test_attribute, has_test_attribute};
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Expr, ExprIf, ExprMatch, Item};

fn is_const_bool(expr: &Expr, value: bool) -> bool {
    matches!(
        expr,
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(b),
            ..
        }) if b.value == value
    )
}

fn visit_macro_call_tokens(
    visitor: &mut ExecutableCallReferenceVisitor<'_>,
    tokens: &proc_macro2::TokenStream,
) {
    if let Some(exprs) = parse_expr_list(tokens) {
        for e in exprs {
            visitor.visit_reachable_expr(&e);
        }
        return;
    }
    if let Some(e) = parse_single_expr(tokens) {
        visitor.visit_reachable_expr(&e);
    }
}

/// Collects call targets that would execute along reachable paths in a `#[test]` body.
struct ExecutableCallReferenceVisitor<'a> {
    refs: &'a mut HashSet<String>,
    qualified: &'a mut HashSet<QualifiedModuleRef>,
}

impl ExecutableCallReferenceVisitor<'_> {
    fn visit_reachable_block(&mut self, block: &syn::Block) {
        for stmt in &block.stmts {
            self.visit_reachable_stmt(stmt);
        }
    }

    fn visit_if(&mut self, i: &ExprIf) {
        if is_const_bool(&i.cond, false) {
            if let Some((_, else_branch)) = &i.else_branch {
                self.visit_reachable_expr(else_branch);
            }
            return;
        }
        if is_const_bool(&i.cond, true) {
            self.visit_reachable_block(&i.then_branch);
            return;
        }
        self.visit_reachable_expr(&i.cond);
        self.visit_reachable_block(&i.then_branch);
        if let Some((_, else_branch)) = &i.else_branch {
            self.visit_reachable_expr(else_branch);
        }
    }

    fn visit_match(&mut self, m: &ExprMatch) {
        self.visit_reachable_expr(&m.expr);
        for arm in &m.arms {
            if let Some((_, guard)) = &arm.guard {
                self.visit_reachable_expr(guard);
            }
            self.visit_reachable_expr(&arm.body);
        }
    }

    fn record_invocation(&mut self, expr: &Expr) {
        match expr {
            Expr::Call(c) => {
                if let Expr::Path(p) = c.func.as_ref() {
                    insert_qualified_path_reference(&p.path, self.refs, self.qualified);
                }
                for arg in &c.args {
                    self.visit_reachable_expr(arg);
                }
            }
            Expr::MethodCall(m) => {
                self.refs.insert(m.method.to_string());
                self.visit_reachable_expr(&m.receiver);
                for arg in &m.args {
                    self.visit_reachable_expr(arg);
                }
            }
            _ => {}
        }
    }

    fn visit_repeat(&mut self, expr: &Expr) {
        match expr {
            Expr::While(w) if is_const_bool(&w.cond, false) => {}
            Expr::While(w) => {
                self.visit_reachable_expr(&w.cond);
                self.visit_reachable_block(&w.body);
            }
            Expr::Loop(l)
                if l.body.stmts.len() == 1
                    && matches!(
                        l.body.stmts.first(),
                        Some(syn::Stmt::Expr(Expr::Break(_), _))
                    ) => {}
            Expr::Loop(l) => self.visit_reachable_block(&l.body),
            _ => {}
        }
    }

    fn visit_reachable_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::If(i) => self.visit_if(i),
            Expr::Match(m) => self.visit_match(m),
            Expr::While(_) | Expr::Loop(_) => self.visit_repeat(expr),
            Expr::Call(_) | Expr::MethodCall(_) => self.record_invocation(expr),
            Expr::Block(b) => self.visit_reachable_block(&b.block),
            Expr::Closure(_) | Expr::Async(_) => {}
            Expr::Macro(m) => visit_macro_call_tokens(self, &m.mac.tokens),
            _ => syn::visit::visit_expr(self, expr),
        }
    }

    fn visit_reachable_stmt(&mut self, stmt: &syn::Stmt) {
        match stmt {
            syn::Stmt::Expr(expr, _) => self.visit_reachable_expr(expr),
            syn::Stmt::Macro(m) => visit_macro_call_tokens(self, &m.mac.tokens),
            syn::Stmt::Local(l) => {
                if let Some(init) = &l.init {
                    self.visit_reachable_expr(&init.expr);
                }
            }
            syn::Stmt::Item(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for ExecutableCallReferenceVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.visit_reachable_expr(expr);
    }
}

pub(super) fn collect_executable_call_references_for_fn(
    f: &syn::ItemFn,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    for stmt in &f.block.stmts {
        ExecutableCallReferenceVisitor { refs, qualified }.visit_reachable_stmt(stmt);
    }
}

pub(super) fn collect_executable_call_references_from_test_fns(
    ast: &syn::File,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    collect_executable_call_references_from_items(&ast.items, "", refs, qualified);
}

fn collect_executable_call_references_from_items(
    items: &[syn::Item],
    prefix: &str,
    refs: &mut HashSet<String>,
    qualified: &mut HashSet<QualifiedModuleRef>,
) {
    for item in items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                let mod_name = m.ident.to_string();
                let mod_prefix = if prefix.is_empty() {
                    mod_name
                } else {
                    format!("{prefix}::{mod_name}")
                };
                if let Some((_, mod_items)) = &m.content {
                    collect_executable_call_references_from_items(
                        mod_items,
                        &mod_prefix,
                        refs,
                        qualified,
                    );
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                collect_executable_call_references_for_fn(f, refs, qualified);
            }
            _ => {}
        }
    }
}

pub(super) fn collect_per_test_call_usage(ast: &syn::File) -> Vec<(String, HashSet<String>)> {
    let mut out = Vec::new();
    collect_per_test_call_usage_from_items(&ast.items, "", &mut out);
    out
}

pub(crate) fn collect_per_test_call_usage_from_items(
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
                    collect_per_test_call_usage_from_items(mod_items, &mod_prefix, out);
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                let fn_name = f.sig.ident.to_string();
                let mut refs = HashSet::new();
                let mut qualified = HashSet::new();
                collect_executable_call_references_for_fn(f, &mut refs, &mut qualified);
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

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::collections::HashSet;
    use syn::visit::Visit;

    impl ExecutableCallReferenceVisitor<'_> {
        fn for_coverage_test<'a>(
            refs: &'a mut HashSet<String>,
            qualified: &'a mut HashSet<QualifiedModuleRef>,
        ) -> ExecutableCallReferenceVisitor<'a> {
            ExecutableCallReferenceVisitor { refs, qualified }
        }
    }

    #[test]
    fn witness_executable_call_visitor() {
        let mut refs = HashSet::new();
        let mut qualified = HashSet::new();
        let mut visitor =
            ExecutableCallReferenceVisitor::for_coverage_test(&mut refs, &mut qualified);
        let expr: syn::Expr = syn::parse_quote!(callee());
        visitor.visit_expr(&expr);
        assert!(refs.contains("callee"));
    }
}
