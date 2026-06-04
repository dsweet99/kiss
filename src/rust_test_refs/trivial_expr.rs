use syn::{Expr, Stmt};

pub(super) fn is_delegation_only_block(block: &syn::Block) -> bool {
    block.stmts.iter().all(is_trivial_stmt)
}

pub(crate) fn is_well_known_constructor(name: &str) -> bool {
    matches!(name, "Ok" | "Err" | "Some" | "None" | "Box" | "Vec")
}

pub(crate) fn is_qualified_or_known_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(c) => {
            if let Expr::Path(p) = c.func.as_ref() {
                let callee_ok = if p.path.segments.len() >= 2 {
                    true
                } else if p.path.segments.len() == 1 {
                    let name = p.path.segments[0].ident.to_string();
                    is_well_known_constructor(&name)
                } else {
                    false
                };
                callee_ok && c.args.iter().all(is_trivial_expr)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn is_trivial_expr_leaf(expr: &Expr) -> bool {
    match expr {
        Expr::Call(_) => is_qualified_or_known_call(expr),
        Expr::Path(_) | Expr::Lit(_) => true,
        Expr::Return(r) => r.expr.as_ref().is_none_or(|e| is_trivial_expr(e)),
        Expr::Try(t) => is_trivial_expr(&t.expr),
        Expr::Await(a) => is_trivial_expr(&a.base),
        Expr::Let(l) => is_trivial_expr(&l.expr),
        _ => false,
    }
}

fn is_trivial_expr_control(expr: &Expr) -> bool {
    match expr {
        Expr::Block(b) => is_delegation_only_block(&b.block),
        Expr::If(i) => {
            is_trivial_expr(&i.cond)
                && is_delegation_only_block(&i.then_branch)
                && i.else_branch
                    .as_ref()
                    .is_none_or(|(_, e)| is_trivial_expr(e))
        }
        Expr::Match(m) => {
            is_trivial_expr(&m.expr)
                && m.arms.iter().all(|arm| {
                    arm.guard.as_ref().is_none_or(|(_, g)| is_trivial_expr(g))
                        && is_trivial_expr(&arm.body)
                })
        }
        _ => false,
    }
}

fn is_trivial_expr_compound(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall(m) => is_trivial_expr(&m.receiver) && m.args.iter().all(is_trivial_expr),
        Expr::Field(f) => is_trivial_expr(&f.base),
        Expr::Reference(r) => is_trivial_expr(&r.expr),
        Expr::Unary(u) => is_trivial_expr(&u.expr),
        Expr::Binary(b) => is_trivial_expr(&b.left) && is_trivial_expr(&b.right),
        Expr::Paren(p) => is_trivial_expr(&p.expr),
        Expr::Tuple(t) => t.elems.iter().all(is_trivial_expr),
        _ => false,
    }
}

pub(crate) fn is_trivial_expr(expr: &Expr) -> bool {
    is_trivial_expr_leaf(expr) || is_trivial_expr_control(expr) || is_trivial_expr_compound(expr)
}

pub(crate) fn is_trivial_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e, _) => is_trivial_expr(e),
        Stmt::Local(l) => l.init.as_ref().is_none_or(|i| is_trivial_expr(&i.expr)),
        Stmt::Item(_) | Stmt::Macro(_) => false,
    }
}
