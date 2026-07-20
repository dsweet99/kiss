use syn::{Block, Expr};

struct BranchCounter(usize);

impl syn::visit::Visit<'_> for BranchCounter {
    fn visit_expr(&mut self, expr: &Expr) {
        if is_branch_expr(expr) {
            self.0 += 1;
        }
        syn::visit::visit_expr(self, expr);
    }
}

fn is_branch_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_)
    )
}

pub(crate) fn count_rs_branches(block: &Block) -> usize {
    let mut counter = BranchCounter(0);
    syn::visit::visit_block(&mut counter, block);
    counter.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_rs_branches_includes_dead_control_flow() {
        let f: syn::ItemFn =
            syn::parse_str("fn t() { if false { a(); } else { b(); } while false { c(); } }")
                .unwrap();
        assert_eq!(count_rs_branches(&f.block), 2);
    }

    #[test]
    fn count_rs_branches_counts_live_loop() {
        let f: syn::ItemFn = syn::parse_str("fn u() { while true { } }").unwrap();
        assert_eq!(count_rs_branches(&f.block), 1);
    }

    #[test]
    fn count_rs_branches_counts_match_for_and_loop() {
        let f: syn::ItemFn =
            syn::parse_str("fn v(xs: &[i32]) { match xs.len() { 0 => {}, _ => {} } for x in xs { let _ = x; } loop { break; } }")
                .unwrap();
        assert_eq!(count_rs_branches(&f.block), 3);
    }

    #[test]
    fn count_rs_branches_counts_nested_control_flow() {
        let f: syn::ItemFn = syn::parse_str(
            "fn nested(xs: &[i32]) { if xs.is_empty() { return; } else { match xs[0] { 0 => while false {}, _ => for x in xs { if *x > 1 { break; } } } } }",
        )
        .unwrap();
        assert_eq!(count_rs_branches(&f.block), 5);
    }

    #[test]
    fn count_rs_branches_returns_zero_for_linear_block() {
        let f: syn::ItemFn = syn::parse_str("fn linear() { let x = 1; let y = x + 1; }").unwrap();
        assert_eq!(count_rs_branches(&f.block), 0);
    }

    #[test]
    fn count_rs_branches_visits_control_flow_inside_closures() {
        let f: syn::ItemFn = syn::parse_str(
            "fn closure_case(flag: bool) { let f = || if flag { loop { break; } } else { while false {} }; f(); }",
        )
        .unwrap();
        assert_eq!(count_rs_branches(&f.block), 3);
    }
}
