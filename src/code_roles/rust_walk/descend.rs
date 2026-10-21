use std::path::Path;

use syn::{Attribute, ItemFn, Stmt};

use super::super::cfg_attr::{attrs_predicate, has_test_or_bench};
use super::super::cfg_pred::{AtomInterner, CfgPred};
use super::super::error::RoleBuildError;
use super::super::rust_include_parse::IncludeKind;
use super::super::rust_walk_attrs::{expr_attrs, impl_item_attrs};
use super::super::span::SourceSpan;
use super::{WalkCtx, WalkOutput};

pub(super) fn walk_fn(
    ctx: &WalkCtx<'_>,
    func: &ItemFn,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = if has_test_or_bench(&func.attrs) {
        pred.clone()
            .and(CfgPred::Atom(super::super::cfg_pred::ATOM_TEST))
    } else {
        pred.clone()
    };
    super::record_span(out, SourceSpan::of_syn(func), &pred, allow_production);
    walk_generics(ctx, &func.sig.generics, &pred, allow_production, atoms, out)?;
    walk_fn_inputs(ctx, &func.sig.inputs, &pred, allow_production, atoms, out)?;
    walk_stmts_ctx(ctx, &func.block.stmts, &pred, allow_production, atoms, out)
}

fn walk_fn_inputs(
    ctx: &WalkCtx<'_>,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    for arg in inputs {
        if let syn::FnArg::Typed(pat) = arg {
            let pred = attrs_predicate(&pat.attrs, inherited, atoms, path)?;
            super::record_span(out, SourceSpan::of_syn(pat), &pred, allow_production);
        }
    }
    Ok(())
}

pub(super) fn walk_generics(
    ctx: &WalkCtx<'_>,
    generics: &syn::Generics,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    for param in &generics.params {
        let attrs = match param {
            syn::GenericParam::Type(t) => t.attrs.as_slice(),
            syn::GenericParam::Lifetime(l) => l.attrs.as_slice(),
            syn::GenericParam::Const(c) => c.attrs.as_slice(),
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        super::record_span(out, SourceSpan::of_syn(param), &pred, allow_production);
    }
    Ok(())
}

pub(crate) fn walk_stmts(
    path: &Path,
    stmts: &[Stmt],
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let ctx = WalkCtx::at_file(path);
    walk_stmts_ctx(&ctx, stmts, inherited, allow_production, atoms, out)
}

fn walk_stmts_ctx(
    ctx: &WalkCtx<'_>,
    stmts: &[Stmt],
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for stmt in stmts {
        walk_stmt(ctx, stmt, inherited, allow_production, atoms, out)?;
    }
    Ok(())
}

fn walk_stmt(
    ctx: &WalkCtx<'_>,
    stmt: &Stmt,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    match stmt {
        Stmt::Item(item) => super::walk_item(ctx, item, inherited, allow_production, atoms, out)?,
        Stmt::Local(local) => {
            let pred = attrs_predicate(&local.attrs, inherited, atoms, path)?;
            super::record_span(out, SourceSpan::of_syn(local), &pred, allow_production);
        }
        Stmt::Expr(expr, _) => walk_expr(ctx, expr, inherited, allow_production, atoms, out)?,
        Stmt::Macro(mac) => {
            let pred = attrs_predicate(&mac.attrs, inherited, atoms, path)?;
            super::record_span(out, SourceSpan::of_syn(mac), &pred, allow_production);
            super::push_include(path, &mac.mac, &pred, IncludeKind::Statements, out);
        }
    }
    Ok(())
}

fn walk_expr(
    ctx: &WalkCtx<'_>,
    expr: &syn::Expr,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    let pred = attrs_predicate(expr_attrs(expr), inherited, atoms, path)?;
    super::record_span(out, SourceSpan::of_syn(expr), &pred, allow_production);
    match expr {
        syn::Expr::Block(block) => {
            walk_stmts_ctx(ctx, &block.block.stmts, &pred, allow_production, atoms, out)?;
        }
        syn::Expr::Macro(mac) => {
            super::push_include(path, &mac.mac, &pred, IncludeKind::Expr, out);
        }
        syn::Expr::If(if_expr) => {
            walk_stmts_ctx(
                ctx,
                &if_expr.then_branch.stmts,
                &pred,
                allow_production,
                atoms,
                out,
            )?;
            if let Some((_, else_expr)) = &if_expr.else_branch {
                walk_expr(ctx, else_expr, &pred, allow_production, atoms, out)?;
            }
        }
        syn::Expr::Match(m) => {
            for arm in &m.arms {
                let arm_pred = attrs_predicate(&arm.attrs, &pred, atoms, path)?;
                super::record_span(out, SourceSpan::of_syn(arm), &arm_pred, allow_production);
                walk_expr(ctx, &arm.body, &arm_pred, allow_production, atoms, out)?;
            }
        }
        syn::Expr::Unsafe(u) => {
            walk_stmts_ctx(ctx, &u.block.stmts, &pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn walk_impl_items(
    ctx: &WalkCtx<'_>,
    imp: &syn::ItemImpl,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    walk_generics(ctx, &imp.generics, inherited, allow_production, atoms, out)?;
    for item in &imp.items {
        match item {
            syn::ImplItem::Fn(func) => {
                let pred = attrs_predicate(&func.attrs, inherited, atoms, path)?;
                let pred = if has_test_or_bench(&func.attrs) {
                    pred.and(CfgPred::Atom(super::super::cfg_pred::ATOM_TEST))
                } else {
                    pred
                };
                super::record_span(out, SourceSpan::of_syn(func), &pred, allow_production);
                walk_generics(ctx, &func.sig.generics, &pred, allow_production, atoms, out)?;
                walk_fn_inputs(ctx, &func.sig.inputs, &pred, allow_production, atoms, out)?;
                walk_stmts_ctx(ctx, &func.block.stmts, &pred, allow_production, atoms, out)?;
            }
            other => {
                let pred = attrs_predicate(impl_item_attrs(other), inherited, atoms, path)?;
                super::record_span(out, SourceSpan::of_syn(other), &pred, allow_production);
            }
        }
    }
    Ok(())
}

pub(super) fn walk_foreign(
    ctx: &WalkCtx<'_>,
    fm: &syn::ItemForeignMod,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    for item in &fm.items {
        let attrs = match item {
            syn::ForeignItem::Fn(f) => f.attrs.as_slice(),
            syn::ForeignItem::Static(s) => s.attrs.as_slice(),
            syn::ForeignItem::Type(t) => t.attrs.as_slice(),
            syn::ForeignItem::Macro(m) => m.attrs.as_slice(),
            _ => &[],
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        super::record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

pub(super) fn walk_trait_items(
    ctx: &WalkCtx<'_>,
    tr: &syn::ItemTrait,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    walk_generics(ctx, &tr.generics, inherited, allow_production, atoms, out)?;
    for item in &tr.items {
        let attrs: &[Attribute] = match item {
            syn::TraitItem::Fn(f) => &f.attrs,
            syn::TraitItem::Type(t) => &t.attrs,
            syn::TraitItem::Const(c) => &c.attrs,
            syn::TraitItem::Macro(m) => &m.attrs,
            _ => continue,
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        super::record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

pub(super) fn walk_variants(
    ctx: &WalkCtx<'_>,
    en: &syn::ItemEnum,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    for variant in &en.variants {
        let pred = attrs_predicate(&variant.attrs, inherited, atoms, path)?;
        super::record_span(out, SourceSpan::of_syn(variant), &pred, allow_production);
    }
    Ok(())
}

pub(super) fn walk_fields(
    ctx: &WalkCtx<'_>,
    fields: &syn::Fields,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    for field in fields {
        let pred = attrs_predicate(&field.attrs, inherited, atoms, path)?;
        super::record_span(out, SourceSpan::of_syn(field), &pred, allow_production);
    }
    Ok(())
}
