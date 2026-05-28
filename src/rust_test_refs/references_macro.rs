use super::{ReferenceVisitor, RefWitnessMode};
use crate::macro_expr_parser::{parse_expr_list, parse_single_expr};
use std::collections::HashSet;
use syn::visit::Visit;

pub(crate) fn try_parse_as_single_expr(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) -> bool {
    if let Some(e) = parse_single_expr(tokens) {
        ReferenceVisitor { refs, mode }.visit_expr(&e);
        return true;
    }
    false
}

pub(crate) fn try_parse_as_expr_list(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) -> bool {
    if let Some(exprs) = parse_expr_list(tokens) {
        for e in exprs {
            ReferenceVisitor { refs, mode }.visit_expr(&e);
        }
        return true;
    }
    false
}

pub(crate) fn visit_nested_token_groups(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    for t in tokens.clone() {
        if let proc_macro2::TokenTree::Group(g) = t {
            visit_macro_tokens(&g.stream(), refs, mode);
        }
    }
}

pub(crate) fn for_each_toplevel_semicolon_segment(
    tokens: &proc_macro2::TokenStream,
    mut visit: impl FnMut(&proc_macro2::TokenStream),
) {
    let mut segment = proc_macro2::TokenStream::new();
    for tree in tokens.clone() {
        if let proc_macro2::TokenTree::Punct(p) = &tree
            && p.as_char() == ';'
        {
            if !segment.is_empty() {
                visit(&segment);
                segment = proc_macro2::TokenStream::new();
            }
            continue;
        }
        segment.extend(std::iter::once(tree));
    }
    if !segment.is_empty() {
        visit(&segment);
    }
}

pub(crate) fn try_parse_semicolon_separated_exprs(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) -> bool {
    let mut parsed = false;
    for_each_toplevel_semicolon_segment(tokens, |seg| {
        if try_parse_as_single_expr(seg, refs, mode) || try_parse_as_expr_list(seg, refs, mode) {
            parsed = true;
        }
    });
    parsed
}

pub(crate) fn visit_macro_tokens(
    tokens: &proc_macro2::TokenStream,
    refs: &mut HashSet<String>,
    mode: RefWitnessMode,
) {
    if try_parse_as_single_expr(tokens, refs, mode) {
        return;
    }
    if try_parse_as_expr_list(tokens, refs, mode) {
        return;
    }
    if try_parse_semicolon_separated_exprs(tokens, refs, mode) {
        return;
    }
    visit_nested_token_groups(tokens, refs, mode);
}

#[cfg(test)]
mod references_macro_coverage {
    use super::*;

    #[test]
    fn semicolon_segment_helpers_cover_all_paths() {
        let mut refs = HashSet::new();
        let tokens: proc_macro2::TokenStream = "Rule::A; not_expr; Rule::B".parse().unwrap();
        visit_macro_tokens(&tokens, &mut refs, RefWitnessMode::COVERAGE_MAP);
        assert!(refs.contains("A"));
        assert!(refs.contains("B"));
        let mut trailing = HashSet::new();
        let tail: proc_macro2::TokenStream = "Rule::TailOnly".parse().unwrap();
        visit_macro_tokens(&tail, &mut trailing, RefWitnessMode::GATE);
        assert!(trailing.contains("TailOnly"));
        let mut empty_seg = HashSet::new();
        let semi_only: proc_macro2::TokenStream = "Rule::X;; Rule::Y".parse().unwrap();
        visit_macro_tokens(&semi_only, &mut empty_seg, RefWitnessMode::COVERAGE_MAP);
        assert!(empty_seg.contains("X"));
        assert!(empty_seg.contains("Y"));
        let mut direct = HashSet::new();
        let tokens: proc_macro2::TokenStream = "Rule::A; bad; Rule::B".parse().unwrap();
        assert!(try_parse_semicolon_separated_exprs(
            &tokens,
            &mut direct,
            RefWitnessMode::COVERAGE_MAP
        ));
        let mut segments = 0usize;
        for_each_toplevel_semicolon_segment(&tokens, |_| segments += 1);
        assert_eq!(segments, 3);
    }
}
