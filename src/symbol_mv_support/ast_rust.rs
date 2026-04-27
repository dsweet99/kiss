//! Rust AST extraction (Task 3).
//!
//! Walks a `syn::File` to enumerate function/method definitions and call
//! sites for `kiss mv`. Rust spans expose only line/column locations (not
//! byte ranges), so byte offsets are derived from line/column via a single
//! prefix scan of the source.

use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprPath, ImplItem, Item, ItemFn, ItemImpl, Type};

use crate::Language;

use super::ast_models::{
    AstResult, Definition, FallbackReason, ParseOutcome, Reference, ReferenceKind, SymbolKind,
};

pub(super) fn parse_rust(content: &str) -> ParseOutcome {
    let Ok(file) = syn::parse_file(content) else {
        return ParseOutcome::Fail(FallbackReason::ParseFailed);
    };
    let line_offsets = compute_line_offsets(content);
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    for item in &file.items {
        collect_rust_item(item, content, &line_offsets, &mut definitions, &mut references);
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
    })
}

pub(super) fn compute_line_offsets(content: &str) -> Vec<usize> {
    let mut offsets = vec![0_usize];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(idx + 1);
        }
    }
    offsets
}

pub(super) fn lc_to_byte(content: &str, line_offsets: &[usize], line: usize, column: usize) -> Option<usize> {
    assert!(line >= 1, "syn line numbers are 1-indexed");
    let row = line.checked_sub(1)?;
    let line_start = *line_offsets.get(row)?;
    let line_end = line_offsets.get(row + 1).copied().unwrap_or(content.len());
    let line_text = &content[line_start..line_end];
    let mut byte_in_line = 0_usize;
    for (chars_seen, ch) in line_text.chars().enumerate() {
        if chars_seen == column {
            return Some(line_start + byte_in_line);
        }
        byte_in_line += ch.len_utf8();
    }
    Some(line_start + byte_in_line)
}

pub(super) fn ident_byte_span(
    line_offsets: &[usize],
    ident: &syn::Ident,
    content: &str,
) -> Option<(usize, usize)> {
    let span = ident.span();
    let start_lc = span.start();
    let start = lc_to_byte(content, line_offsets, start_lc.line, start_lc.column)?;
    let name = ident.to_string();
    let end = start + name.len();
    if end <= content.len() && content.is_char_boundary(start) && content[start..end] == name {
        return Some((start, end));
    }
    None
}

pub(super) fn collect_rust_item(
    item: &Item,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    match item {
        Item::Fn(item_fn) => collect_top_fn(item_fn, content, line_offsets, defs, refs),
        Item::Impl(item_impl) => collect_impl(item_impl, content, line_offsets, defs, refs),
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
                    collect_rust_item(nested, content, line_offsets, defs, refs);
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
    };
    visitor.visit_item_fn(item_fn);
}

pub(super) fn item_full_span<T: Spanned>(
    item: &T,
    content: &str,
    line_offsets: &[usize],
) -> Option<(usize, usize)> {
    let span = item.span();
    let start_lc = span.start();
    let end_lc = span.end();
    let start = lc_to_byte(content, line_offsets, start_lc.line, start_lc.column)?;
    let end = lc_to_byte(content, line_offsets, end_lc.line, end_lc.column)?;
    if end > start && end <= content.len() {
        Some((start, end))
    } else {
        None
    }
}

pub(super) fn collect_impl(
    item_impl: &ItemImpl,
    content: &str,
    line_offsets: &[usize],
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    let owner = impl_owner_name(&item_impl.self_ty);
    for impl_item in &item_impl.items {
        if let ImplItem::Fn(method) = impl_item {
            if let Some((s, e)) = item_full_span(method, content, line_offsets)
                && let Some((ns, ne)) =
                    ident_byte_span(line_offsets, &method.sig.ident, content)
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
            }
            let mut visitor = CallVisitor {
                content,
                line_offsets,
                refs,
            };
            visitor.visit_block(&method.block);
        }
    }
}

const WRAPPER_TYPES: &[&str] = &["Box", "Vec", "Arc", "Rc", "Pin", "Cow", "RefCell", "Cell"];

pub(super) fn impl_owner_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(r) => impl_owner_name(&r.elem),
        Type::Group(g) => impl_owner_name(&g.elem),
        Type::Paren(p) => impl_owner_name(&p.elem),
        Type::Path(tp) => {
            let seg = tp.path.segments.last()?;
            let name = seg.ident.to_string();
            if WRAPPER_TYPES.contains(&name.as_str())
                && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
            {
                for arg in &args.args {
                    if let syn::GenericArgument::Type(inner) = arg {
                        return impl_owner_name(inner);
                    }
                }
            }
            Some(name)
        }
        _ => None,
    }
}

struct NestedDefVisitor<'a> {
    content: &'a str,
    line_offsets: &'a [usize],
    defs: &'a mut Vec<Definition>,
    depth: usize,
}

impl<'ast> Visit<'ast> for NestedDefVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.depth > 0
            && let Some((s, e)) = item_full_span(node, self.content, self.line_offsets)
            && let Some((ns, ne)) =
                ident_byte_span(self.line_offsets, &node.sig.ident, self.content)
        {
            self.defs.push(Definition {
                name: node.sig.ident.to_string(),
                owner: None,
                kind: SymbolKind::Function,
                start: s,
                end: e,
                name_start: ns,
                name_end: ne,
                language: Language::Rust,
            });
        }
        self.depth += 1;
        syn::visit::visit_item_fn(self, node);
        self.depth -= 1;
    }
}

struct CallVisitor<'a> {
    content: &'a str,
    line_offsets: &'a [usize],
    refs: &'a mut Vec<Reference>,
}

impl<'ast> Visit<'ast> for CallVisitor<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(ExprPath { path, .. }) = node.func.as_ref()
            && let Some(seg) = path.segments.last()
            && let Some((s, e)) = ident_byte_span(self.line_offsets, &seg.ident, self.content)
        {
            self.refs.push(Reference {
                start: s,
                end: e,
                kind: ReferenceKind::Call,
            });
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if let Some((s, e)) = ident_byte_span(self.line_offsets, &node.method, self.content) {
            self.refs.push(Reference {
                start: s,
                end: e,
                kind: ReferenceKind::Method,
            });
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_use_path(&mut self, node: &'ast syn::UsePath) {
        if let Some((s, e)) = ident_byte_span(self.line_offsets, &node.ident, self.content) {
            self.refs.push(Reference {
                start: s,
                end: e,
                kind: ReferenceKind::Import,
            });
        }
        syn::visit::visit_use_path(self, node);
    }

    fn visit_use_name(&mut self, node: &'ast syn::UseName) {
        self.push_use_ident(&node.ident);
    }

    fn visit_use_rename(&mut self, node: &'ast syn::UseRename) {
        self.push_use_ident(&node.ident);
    }
}

impl CallVisitor<'_> {
    fn push_use_ident(&mut self, ident: &syn::Ident) {
        if let Some((s, e)) = ident_byte_span(self.line_offsets, ident, self.content) {
            self.refs.push(Reference {
                start: s,
                end: e,
                kind: ReferenceKind::Import,
            });
        }
    }
}

#[cfg(test)]
#[path = "ast_rust_test.rs"]
mod ast_rust_test;

