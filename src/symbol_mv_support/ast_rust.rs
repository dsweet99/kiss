#[cfg(test)]
#[path = "ast_rust_test.rs"]
mod ast_rust_test;

// Rust AST extraction (Task 3).
//
// Walks a `syn::File` to enumerate function/method definitions and call
// sites for `kiss mv`. Rust spans expose only line/column locations (not
// byte ranges), so byte offsets are derived from line/column via a single
// prefix scan of the source.
use super::ast_models::{
    AstResult, Definition, FallbackReason, ParseOutcome, Reference, SymbolKind, TraitImpl,
};
use super::ast_rust_span::{compute_line_offsets, ident_byte_span, item_full_span};
use super::ast_rust_visitors::{CallVisitor, NestedDefVisitor, collect_impl};
pub(super) use super::ast_rust_visitors::impl_owner_name;
use crate::Language;
use syn::visit::Visit;
use syn::{Item, ItemFn};
pub(super) fn parse_rust(content: &str) -> ParseOutcome {
    let Ok(file) = syn::parse_file(content) else {
        return ParseOutcome::Fail(FallbackReason::ParseFailed);
    };
    let line_offsets = compute_line_offsets(content);
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut trait_impls = Vec::new();
    for item in &file.items {
        collect_rust_item(
            item,
            content,
            &line_offsets,
            &mut definitions,
            &mut references,
            &mut trait_impls,
        );
    }
    let mut nested = NestedDefVisitor {
        content,
        line_offsets: &line_offsets,
        defs: &mut definitions,
        depth: 0,
    };
    syn::visit::visit_file(&mut nested, &file);
    ParseOutcome::Success(AstResult {
        definitions,
        references,
        trait_impls,
    })
}

pub(super) fn collect_rust_item(
    item: &Item,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
    trait_impls: &mut Vec<TraitImpl>,
) {
    match item {
        Item::Fn(item_fn) => collect_top_fn(item_fn, content, line_offsets, defs, refs),
        Item::Impl(item_impl) => {
            collect_impl(item_impl, content, line_offsets, defs, refs);
            if let (Some((_, trait_path, _)), Some(implementor)) =
                (&item_impl.trait_, impl_owner_name(&item_impl.self_ty))
                && let Some(seg) = trait_path.segments.last()
            {
                trait_impls.push(TraitImpl {
                    trait_name: seg.ident.to_string(),
                    implementor,
                });
            }
        }
        Item::Use(item_use) => collect_use(item_use, content, line_offsets, refs),
        Item::Trait(item_trait) => {
            collect_trait(item_trait, content, line_offsets, defs, refs);
        }
        Item::ForeignMod(item_foreign) => {
            collect_foreign_mod(item_foreign, content, line_offsets, defs);
        }
        Item::Mod(m) => {
            if let Some((_, items)) = &m.content {
                for nested in items {
                    collect_rust_item(nested, content, line_offsets, defs, refs, trait_impls);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn collect_trait(
    item_trait: &syn::ItemTrait,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    let owner = Some(item_trait.ident.to_string());
    for trait_item in &item_trait.items {
        if let syn::TraitItem::Fn(method) = trait_item
            && let Some((s, e)) = item_full_span(method, content, line_offsets)
            && let Some((ns, ne)) = ident_byte_span(line_offsets, &method.sig.ident, content)
        {
            defs.push(Definition {
                name: method.sig.ident.to_string(),
                owner: owner.clone(),
                kind: SymbolKind::Method,
                start: s,
                end: e,
                name_start: ns,
                name_end: ne,
                language: Language::Rust,
            });
            if let Some(default_block) = &method.default {
                let mut visitor = CallVisitor {
                    content,
                    line_offsets,
                    refs,
                    in_call: false,
                };
                visitor.visit_block(default_block);
            }
        }
    }
}

pub(super) fn collect_foreign_mod(
    item_foreign: &syn::ItemForeignMod,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
) {
    for foreign in &item_foreign.items {
        if let syn::ForeignItem::Fn(f) = foreign
            && let Some((s, e)) = item_full_span(f, content, line_offsets)
            && let Some((ns, ne)) = ident_byte_span(line_offsets, &f.sig.ident, content)
        {
            defs.push(Definition {
                name: f.sig.ident.to_string(),
                owner: None,
                kind: SymbolKind::Function,
                start: s,
                end: e,
                name_start: ns,
                name_end: ne,
                language: Language::Rust,
            });
        }
    }
}

pub(super) fn collect_use(
    item_use: &syn::ItemUse,
    content: &str,
    line_offsets: &[usize],
    refs: &mut Vec<Reference>,
) {
    let mut visitor = CallVisitor {
        content,
        line_offsets,
        refs,
        in_call: false,
    };
    visitor.visit_item_use(item_use);
}

pub(super) fn collect_top_fn(
    item_fn: &ItemFn,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    if let Some((s, e)) = item_full_span(item_fn, content, line_offsets)
        && let Some((ns, ne)) = ident_byte_span(line_offsets, &item_fn.sig.ident, content)
    {
        defs.push(Definition {
            name: item_fn.sig.ident.to_string(),
            owner: None,
            kind: SymbolKind::Function,
            start: s,
            end: e,
            name_start: ns,
            name_end: ne,
            language: Language::Rust,
        });
    }
    let mut visitor = CallVisitor {
        content,
        line_offsets,
        refs,
        in_call: false,
    };
    visitor.visit_item_fn(item_fn);
}

