use syn::visit::Visit;

use super::super::ast_models::Reference;
use super::CallVisitor;
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};
pub(super) fn collect_macro_reference_sites(
    tokens: &proc_macro2::TokenStream,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) {
    if try_parse_as_single_expr(tokens, content, line_offsets, refs) {
        return;
    }
    if try_parse_as_expr_list(tokens, content, line_offsets, refs) {
        return;
    }
    visit_nested_token_groups(tokens, content, line_offsets, refs);
}

fn try_parse_as_single_expr(
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
        };
        visitor.visit_expr(&expr);
        return true;
    }
    false
}

fn try_parse_as_expr_list(
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
        };
        for expr in exprs {
            visitor.visit_expr(&expr);
        }
        return true;
    }
    false
}

fn visit_nested_token_groups(
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
