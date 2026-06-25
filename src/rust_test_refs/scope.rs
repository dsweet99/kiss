use syn::{Block, Expr};

pub(crate) fn count_rs_branches(block: &Block) -> usize {
    struct Counter {
        count: usize,
    }
    impl syn::visit::Visit<'_> for Counter {
        fn visit_expr(&mut self, expr: &Expr) {
            if matches!(
                expr,
                Expr::If(_) | Expr::Match(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_)
            ) {
                self.count += 1;
            }
            syn::visit::visit_expr(self, expr);
        }
    }
    let mut counter = Counter { count: 0 };
    syn::visit::visit_block(&mut counter, block);
    counter.count
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
}
