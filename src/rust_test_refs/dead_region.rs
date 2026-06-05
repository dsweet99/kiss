use syn::{BinOp, Expr, ExprBinary, UnOp};
use syn::visit::Visit;

pub(crate) fn is_rs_const_false(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(l) => match &l.lit {
            syn::Lit::Bool(b) => !b.value,
            syn::Lit::Int(i) => i.base10_parse::<i64>().ok() == Some(0),
            _ => false,
        },
        Expr::Unary(u) if matches!(u.op, UnOp::Not(_)) => is_rs_const_true(&u.expr),
        Expr::Binary(b) => !eval_rs_comparison(b),
        Expr::Path(p) if p.path.is_ident("false") => true,
        _ => false,
    }
}

fn is_rs_const_true(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(l) => match &l.lit {
            syn::Lit::Bool(b) => b.value,
            syn::Lit::Int(i) => i.base10_parse::<i64>().ok().is_some_and(|v| v != 0),
            _ => false,
        },
        Expr::Unary(u) if matches!(u.op, UnOp::Not(_)) => is_rs_const_false(&u.expr),
        Expr::Binary(b) => eval_rs_comparison(b),
        Expr::Path(p) if p.path.is_ident("true") => true,
        _ => false,
    }
}

fn eval_rs_comparison(b: &ExprBinary) -> bool {
    let (Some(l), Some(r)) = (rs_literal_i64(&b.left), rs_literal_i64(&b.right)) else {
        return false;
    };
    match b.op {
        BinOp::Eq(_) => l == r,
        BinOp::Ne(_) => l != r,
        BinOp::Lt(_) => l < r,
        BinOp::Gt(_) => l > r,
        BinOp::Le(_) => l <= r,
        BinOp::Ge(_) => l >= r,
        _ => false,
    }
}

fn rs_literal_i64(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Lit(l) => match &l.lit {
            syn::Lit::Int(i) => i.base10_parse().ok(),
            syn::Lit::Bool(b) => Some(i64::from(b.value)),
            _ => None,
        },
        Expr::Path(p) if p.path.is_ident("true") => Some(1),
        Expr::Path(p) if p.path.is_ident("false") => Some(0),
        _ => None,
    }
}

pub(crate) fn skip_dead_control_flow<'a, V: syn::visit::Visit<'a>>(
    visitor: &mut V,
    expr: &'a Expr,
) -> bool {
    if let Expr::If(i) = expr {
        if is_rs_const_false(&i.cond) {
            if let Some((_, else_branch)) = &i.else_branch {
                visitor.visit_expr(else_branch);
            }
        } else {
            visitor.visit_block(&i.then_branch);
            if let Some((_, else_branch)) = &i.else_branch {
                visitor.visit_expr(else_branch);
            }
        }
        return true;
    }
    if let Expr::While(w) = expr {
        return is_rs_const_false(&w.cond);
    }
    false
}

pub(crate) fn count_rs_live_branches(block: &syn::Block) -> usize {
    struct Counter {
        count: usize,
    }
    impl<'ast> syn::visit::Visit<'ast> for Counter {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if skip_dead_control_flow(self, expr) {
                return;
            }
            if matches!(
                expr,
                Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_)
            ) {
                self.count += 1;
            }
            syn::visit::visit_expr(self, expr);
        }

        fn visit_expr_if(&mut self, i: &'ast syn::ExprIf) {
            if is_rs_const_false(&i.cond) {
                if let Some((_, else_branch)) = &i.else_branch {
                    self.visit_expr(else_branch);
                }
                return;
            }
            self.count += 1;
            self.visit_block(&i.then_branch);
            if let Some((_, else_branch)) = &i.else_branch {
                self.visit_expr(else_branch);
            }
        }
    }
    let mut counter = Counter { count: 0 };
    counter.visit_block(block);
    counter.count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rs_const_false_detects_false_and_zero() {
        assert!(is_rs_const_false(&syn::parse_str("false").unwrap()));
        assert!(is_rs_const_false(&syn::parse_str("0").unwrap()));
        assert!(!is_rs_const_false(&syn::parse_str("true").unwrap()));
    }

    #[test]
    fn dead_if_refs_not_collected() {
        let f: syn::ItemFn = syn::parse_str(
            "fn test_fn() { if false { foo(); } bar(); }",
        )
        .unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("bar"));
        assert!(!refs.contains("foo"));
    }

    #[test]
    fn while_false_skipped_for_branches() {
        let f: syn::ItemFn = syn::parse_str("fn t() { while false { break; } real(); }").unwrap();
        let n = count_rs_live_branches(&f.block);
        assert_eq!(n, 0);
    }

    #[test]
    fn if_false_else_branch_still_live() {
        let f: syn::ItemFn =
            syn::parse_str("fn t() { if false { dead(); } else { live(); } }").unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("live"));
        assert!(!refs.contains("dead"));
    }

    #[test]
    fn skip_dead_control_flow_if_true_visits_then_branch() {
        let f: syn::ItemFn = syn::parse_str("fn t() { if true { live(); } }").unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("live"));
    }

    #[test]
    fn comparison_const_false() {
        assert!(is_rs_const_false(&syn::parse_str("1 == 2").unwrap()));
        assert!(!is_rs_const_false(&syn::parse_str("1 == 1").unwrap()));
    }

    #[test]
    fn not_true_is_false() {
        assert!(is_rs_const_false(&syn::parse_str("!true").unwrap()));
    }

    #[test]
    fn skip_dead_for_loop_false() {
        let f: syn::ItemFn =
            syn::parse_str("fn t() { for _ in [] { dead(); } live(); }").unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("live"));
    }

    #[test]
    fn skip_dead_if_with_else_expr_branch() {
        let f: syn::ItemFn = syn::parse_str(
            "fn t() { if false { dead(); } else if false { dead2(); } else { live(); } }",
        )
        .unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("live"));
        assert!(!refs.contains("dead"));
        assert!(!refs.contains("dead2"));
    }

    #[test]
    fn skip_dead_while_true_still_visits_body() {
        let f: syn::ItemFn = syn::parse_str("fn t() { while true { inner(); } }").unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("inner"));
    }

    #[test]
    fn skip_dead_if_false_without_else() {
        let f: syn::ItemFn = syn::parse_str("fn t() { if false { dead(); } live(); }").unwrap();
        let mut refs = std::collections::HashSet::new();
        struct RefVisitor<'a> {
            refs: &'a mut std::collections::HashSet<String>,
        }
        impl<'ast> syn::visit::Visit<'ast> for RefVisitor<'_> {
            fn visit_expr(&mut self, expr: &'ast Expr) {
                if skip_dead_control_flow(self, expr) {
                    return;
                }
                if let Expr::Path(p) = expr
                    && let Some(id) = p.path.get_ident()
                {
                    self.refs.insert(id.to_string());
                }
                syn::visit::visit_expr(self, expr);
            }
        }
        RefVisitor { refs: &mut refs }.visit_item_fn(&f);
        assert!(refs.contains("live"));
        assert!(!refs.contains("dead"));
    }

    #[test]
    fn direct_all_const_eval_helpers() {
        let false_expr: Expr = syn::parse_str("false").unwrap();
        let true_expr: Expr = syn::parse_str("true").unwrap();
        let zero: Expr = syn::parse_str("0").unwrap();
        let cmp: ExprBinary = syn::parse_str("1 == 2").unwrap();
        assert!(is_rs_const_false(&false_expr));
        assert!(is_rs_const_false(&zero));
        assert!(!is_rs_const_false(&true_expr));
        assert!(is_rs_const_true(&true_expr));
        assert!(!is_rs_const_true(&false_expr));
        assert!(!eval_rs_comparison(&cmp));
        assert_eq!(rs_literal_i64(&zero), Some(0));
        let f: syn::ItemFn = syn::parse_str(
            "fn t() { if false { a(); } else { b(); } while false { c(); } }",
        )
        .unwrap();
        struct V;
        impl syn::visit::Visit<'_> for V {
            fn visit_expr(&mut self, expr: &Expr) {
                let _ = skip_dead_control_flow(self, expr);
                syn::visit::visit_expr(self, expr);
            }
        }
        V.visit_item_fn(&f);
        assert_eq!(count_rs_live_branches(&f.block), 0);
        let live: syn::ItemFn = syn::parse_str("fn u() { while true { } }").unwrap();
        assert_eq!(count_rs_live_branches(&live.block), 1);
    }
}
