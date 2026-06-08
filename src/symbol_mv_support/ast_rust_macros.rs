use syn::visit::Visit;

use super::ast_models::Reference;
use super::ast_rust_visitors::CallVisitor;
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};

pub(super) fn collect_macro_reference_sites(
    tokens: &proc_macro2::TokenStream,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) {
    if parse_macro_as_single_expr(tokens, content, line_offsets, refs) {
        return;
    }
    if parse_macro_as_expr_list(tokens, content, line_offsets, refs) {
        return;
    }
    visit_macro_nested_token_groups(tokens, content, line_offsets, refs);
}

fn parse_macro_as_single_expr(
    tokens: &proc_macro2::TokenStream,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) -> bool {
    if let Some(expr) = parse_single_expr(tokens) {
        let mut visitor = CallVisitor {
            content,
            line_offsets,
            refs,
            in_call: false,
        };
        visitor.visit_expr(&expr);
        return true;
    }
    false
}

fn parse_macro_as_expr_list(
    tokens: &proc_macro2::TokenStream,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) -> bool {
    if let Some(exprs) = parse_expr_list(tokens) {
        let mut visitor = CallVisitor {
            content,
            line_offsets,
            refs,
            in_call: false,
        };
        for expr in exprs {
            visitor.visit_expr(&expr);
        }
        return true;
    }
    false
}

fn visit_macro_nested_token_groups(
    tokens: &proc_macro2::TokenStream,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) {
    for token in tokens.clone() {
        if let proc_macro2::TokenTree::Group(group) = token {
            collect_macro_reference_sites(&group.stream(), content, line_offsets, refs);
        }
    }
}

#[cfg(test)]
mod ast_rust_macros_tests {
    use crate::symbol_mv_support::ast_models::Reference;
    use super::*;

    #[test]
    fn macro_parser_helpers_visit_token_streams() {
        let single: proc_macro2::TokenStream = "helper()".parse().unwrap();
        let mut single_refs = Vec::<Reference>::new();
        assert!(parse_macro_as_single_expr(
            &single,
            "helper()",
            &[0],
            &mut single_refs
        ));
        assert!(!single_refs.is_empty());

        let list: proc_macro2::TokenStream = "a, b".parse().unwrap();
        let mut list_refs = Vec::<Reference>::new();
        assert!(parse_macro_as_expr_list(&list, "a, b", &[0], &mut list_refs));
        assert!(!list_refs.is_empty());

        let nested: proc_macro2::TokenStream = "(helper())".parse().unwrap();
        let mut nested_refs = Vec::<Reference>::new();
        visit_macro_nested_token_groups(&nested, "(helper())", &[0], &mut nested_refs);
        assert!(!nested_refs.is_empty());
        collect_macro_reference_sites(&single, "helper()", &[0], &mut single_refs);
        assert!(single_refs.len() >= 2);
    }
}
