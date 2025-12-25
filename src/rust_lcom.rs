//! LCOM (Lack of Cohesion of Methods) computation for Rust impl blocks

use std::collections::HashSet;
use syn::visit::Visit;
use syn::{Block, Expr, ImplItem};

/// Compute LCOM for a Rust impl block.
/// LCOM = `pairs_not_sharing_fields` / `total_pairs`. Returns 0.0 (cohesive) to 1.0 (no cohesion).
#[must_use]
pub fn compute_rust_lcom(impl_block: &syn::ItemImpl) -> f64 {
    const MIN_METHODS_FOR_LCOM: usize = 2;

    let fields_per_method: Vec<HashSet<String>> = impl_block
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) => Some(extract_self_field_accesses(&method.block)),
            _ => None,
        })
        .collect();

    if fields_per_method.len() < MIN_METHODS_FOR_LCOM {
        return 0.0;
    }

    let (pairs_sharing, pairs_not_sharing) = count_field_sharing_pairs(&fields_per_method);
    let total_pairs = pairs_sharing + pairs_not_sharing;

    if total_pairs == 0 {
        0.0
    } else {
        pairs_not_sharing as f64 / total_pairs as f64
    }
}

pub fn count_field_sharing_pairs(fields_per_method: &[HashSet<String>]) -> (usize, usize) {
    let mut sharing = 0;
    let mut not_sharing = 0;
    for i in 0..fields_per_method.len() {
        for j in (i + 1)..fields_per_method.len() {
            if fields_per_method[i]
                .intersection(&fields_per_method[j])
                .next()
                .is_some()
            {
                sharing += 1;
            } else {
                not_sharing += 1;
            }
        }
    }
    (sharing, not_sharing)
}

/// Extract all self.field accesses from a block
pub fn extract_self_field_accesses(block: &Block) -> HashSet<String> {
    struct FieldAccessVisitor {
        fields: HashSet<String>,
    }

    impl<'ast> Visit<'ast> for FieldAccessVisitor {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if let Expr::Field(field_expr) = expr
                && let Expr::Path(path_expr) = &*field_expr.base
                && path_expr.path.is_ident("self")
                && let syn::Member::Named(ident) = &field_expr.member
            {
                self.fields.insert(ident.to_string());
            }
            syn::visit::visit_expr(self, expr);
        }
    }

    let mut visitor = FieldAccessVisitor {
        fields: HashSet::new(),
    };
    visitor.visit_block(block);
    visitor.fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_rust_lcom() {
        let f: syn::File =
            syn::parse_str("struct S { x: i32 } impl S { fn a(&self) { let _=self.x; } }").unwrap();
        if let syn::Item::Impl(i) = &f.items[1] {
            assert!(compute_rust_lcom(i) <= 1.0);
        }
    }

    #[test]
    fn test_count_field_sharing_pairs() {
        let m1: HashSet<String> = ["x"].into_iter().map(String::from).collect();
        let m2: HashSet<String> = ["x"].into_iter().map(String::from).collect();
        let m3: HashSet<String> = ["y"].into_iter().map(String::from).collect();
        let (sharing, not_sharing) = count_field_sharing_pairs(&[m1, m2, m3]);
        assert_eq!(sharing, 1);
        assert_eq!(not_sharing, 2);
    }

    #[test]
    fn test_extract_self_field_accesses() {
        let f: syn::File = syn::parse_str("fn foo() { let _=self.x; }").unwrap();
        if let syn::Item::Fn(func) = &f.items[0] {
            let _ = extract_self_field_accesses(&func.block);
        }
    }
}

