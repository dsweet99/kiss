//! `impl` block / visitor handling split out of `ast_rust.rs` to keep that
//! file under the `lines_per_file` gate.

use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMacro, ExprPath, ImplItem, ItemFn, ItemImpl, Type};

use super::super::ast_models::{Definition, Reference, ReferenceKind, SymbolKind};
use crate::Language;
use super::{ident_byte_span, item_full_span};
use super::ast_rust_macros::collect_macro_reference_sites;

pub(crate) fn collect_impl(
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
            }
            let mut visitor = CallVisitor {
                content,
                line_offsets,
                refs,
                in_call: false,
            };
            visitor.visit_block(&method.block);
        }
    }
}

const WRAPPER_TYPES: &[&str] = &["Box", "Vec", "Arc", "Rc", "Pin", "Cow", "RefCell", "Cell"];

pub(crate) fn impl_owner_name(ty: &Type) -> Option<String> {
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

pub(crate) struct NestedDefVisitor<'a> {
    pub(super) content: &'a str,
    pub(super) line_offsets: &'a [usize],
    pub(super) defs: &'a mut Vec<Definition>,
    pub(super) depth: usize,
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

pub(crate) struct CallVisitor<'a> {
    pub(super) content: &'a str,
    pub(super) line_offsets: &'a [usize],
    pub(super) refs: &'a mut Vec<Reference>,
    pub(super) in_call: bool,
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
        let saved_in_call = self.in_call;
        self.in_call = true;
        self.visit_expr(&node.func);
        self.in_call = saved_in_call;
        for arg in &node.args {
            self.visit_expr(arg);
        }
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        collect_macro_reference_sites(&node.mac.tokens, self.content, self.line_offsets, self.refs);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        collect_macro_reference_sites(&node.mac.tokens, self.content, self.line_offsets, self.refs);
    }

    /// Emit a Call reference for any `Expr::Path` value-use (function
    /// pointers, callback args, struct field initializers, etc.) — fixes
    /// KPOP round 6 H2 (Rust "function-as-value not renamed"). Direct
    /// `foo()` calls also descend through here via the default
    /// `visit_expr_call` recursion; the planner dedupes by (start, end).
    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if self.in_call {
            return;
        }
        if let Some(seg) = node.path.segments.last()
            && let Some((s, e)) = ident_byte_span(self.line_offsets, &seg.ident, self.content)
        {
            self.refs.push(Reference {
                start: s,
                end: e,
                kind: ReferenceKind::Call,
            });
        }
        syn::visit::visit_expr_path(self, node);
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
