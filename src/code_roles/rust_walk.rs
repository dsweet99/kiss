use std::path::{Path, PathBuf};

use syn::{Attribute, Item, ItemFn, ItemMod, Stmt};

use crate::rust_include::{extract_include_literal_from_macro, resolve_include_path};

use super::cfg_attr::{attrs_predicate, file_inner_predicate, has_test_or_bench};
use super::cfg_pred::{AtomInterner, CfgPred};
use super::cfg_sat::contexts_for_pred;
use super::error::RoleBuildError;
use super::facts::RoleRange;
use super::rust_include_parse::IncludeKind;
use super::rust_modules::{ModEdge, resolve_external_mod};
use super::rust_walk_attrs::{expr_attrs, impl_item_attrs, item_attrs};
use super::span::SourceSpan;

pub struct WalkOutput {
    pub ranges: Vec<RoleRange>,
    pub mods: Vec<ModEdge>,
    pub includes: Vec<(PathBuf, CfgPred, IncludeKind)>,
}

pub fn walk_file(
    path: &Path,
    ast: &syn::File,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
) -> Result<WalkOutput, RoleBuildError> {
    let pred = file_inner_predicate(&ast.attrs, inherited, atoms, path)?;
    let mut out = WalkOutput {
        ranges: Vec::new(),
        mods: Vec::new(),
        includes: Vec::new(),
    };
    walk_items(path, &ast.items, &pred, allow_production, atoms, &mut out)?;
    Ok(out)
}

pub fn walk_items(
    path: &Path,
    items: &[Item],
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for item in items {
        walk_item(path, item, inherited, allow_production, atoms, out)?;
    }
    Ok(())
}

fn walk_item(
    path: &Path,
    item: &Item,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = attrs_predicate(item_attrs(item), inherited, atoms, path)?;
    record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    walk_item_body(path, item, &pred, allow_production, atoms, out)
}

fn walk_item_body(
    path: &Path,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match item {
        Item::Mod(module) => walk_mod(path, module, pred, allow_production, atoms, out)?,
        Item::Fn(func) => walk_fn(path, func, pred, allow_production, atoms, out)?,
        Item::Impl(imp) => walk_impl_items(path, imp, pred, allow_production, atoms, out)?,
        Item::Trait(tr) => walk_trait_items(path, tr, pred, allow_production, atoms, out)?,
        Item::ForeignMod(fm) => walk_foreign(path, fm, pred, allow_production, atoms, out)?,
        Item::Macro(mac) => push_include(path, &mac.mac, pred, IncludeKind::Items, out),
        other => walk_data_item(path, other, pred, allow_production, atoms, out)?,
    }
    Ok(())
}

fn walk_data_item(
    path: &Path,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match item {
        Item::Enum(en) => {
            walk_generics(path, &en.generics, pred, allow_production, atoms, out)?;
            walk_variants(path, en, pred, allow_production, atoms, out)?;
        }
        Item::Struct(st) => {
            walk_generics(path, &st.generics, pred, allow_production, atoms, out)?;
            walk_fields(path, &st.fields, pred, allow_production, atoms, out)?;
        }
        Item::Union(un) => {
            walk_generics(path, &un.generics, pred, allow_production, atoms, out)?;
            let fields = syn::Fields::Named(un.fields.clone());
            walk_fields(path, &fields, pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_mod(
    path: &Path,
    module: &ItemMod,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    if let Some((_, items)) = &module.content {
        walk_items(path, items, pred, allow_production, atoms, out)?;
    } else {
        out.mods
            .extend(resolve_external_mod(path, module, pred, atoms)?);
    }
    Ok(())
}

fn walk_fn(
    path: &Path,
    func: &ItemFn,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = if has_test_or_bench(&func.attrs) {
        pred.clone().and(CfgPred::Atom(super::cfg_pred::ATOM_TEST))
    } else {
        pred.clone()
    };
    record_span(out, SourceSpan::of_syn(func), &pred, allow_production);
    walk_generics(
        path,
        &func.sig.generics,
        &pred,
        allow_production,
        atoms,
        out,
    )?;
    walk_fn_inputs(path, &func.sig.inputs, &pred, allow_production, atoms, out)?;
    walk_stmts(path, &func.block.stmts, &pred, allow_production, atoms, out)
}

fn walk_fn_inputs(
    path: &Path,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for arg in inputs {
        if let syn::FnArg::Typed(pat) = arg {
            let pred = attrs_predicate(&pat.attrs, inherited, atoms, path)?;
            record_span(out, SourceSpan::of_syn(pat), &pred, allow_production);
        }
    }
    Ok(())
}

fn walk_generics(
    path: &Path,
    generics: &syn::Generics,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for param in &generics.params {
        let attrs = match param {
            syn::GenericParam::Type(t) => t.attrs.as_slice(),
            syn::GenericParam::Lifetime(l) => l.attrs.as_slice(),
            syn::GenericParam::Const(c) => c.attrs.as_slice(),
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        record_span(out, SourceSpan::of_syn(param), &pred, allow_production);
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
    for stmt in stmts {
        walk_stmt(path, stmt, inherited, allow_production, atoms, out)?;
    }
    Ok(())
}

fn walk_stmt(
    path: &Path,
    stmt: &Stmt,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match stmt {
        Stmt::Item(item) => walk_item(path, item, inherited, allow_production, atoms, out)?,
        Stmt::Local(local) => {
            let pred = attrs_predicate(&local.attrs, inherited, atoms, path)?;
            record_span(out, SourceSpan::of_syn(local), &pred, allow_production);
        }
        Stmt::Expr(expr, _) => walk_expr(path, expr, inherited, allow_production, atoms, out)?,
        Stmt::Macro(mac) => {
            let pred = attrs_predicate(&mac.attrs, inherited, atoms, path)?;
            record_span(out, SourceSpan::of_syn(mac), &pred, allow_production);
            push_include(path, &mac.mac, &pred, IncludeKind::Statements, out);
        }
    }
    Ok(())
}

fn walk_expr(
    path: &Path,
    expr: &syn::Expr,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = attrs_predicate(expr_attrs(expr), inherited, atoms, path)?;
    record_span(out, SourceSpan::of_syn(expr), &pred, allow_production);
    match expr {
        syn::Expr::Block(block) => {
            walk_stmts(
                path,
                &block.block.stmts,
                &pred,
                allow_production,
                atoms,
                out,
            )?;
        }
        syn::Expr::Macro(mac) => {
            push_include(path, &mac.mac, &pred, IncludeKind::Expr, out);
        }
        syn::Expr::If(if_expr) => {
            walk_stmts(
                path,
                &if_expr.then_branch.stmts,
                &pred,
                allow_production,
                atoms,
                out,
            )?;
            if let Some((_, else_expr)) = &if_expr.else_branch {
                walk_expr(path, else_expr, &pred, allow_production, atoms, out)?;
            }
        }
        syn::Expr::Match(m) => {
            for arm in &m.arms {
                let arm_pred = attrs_predicate(&arm.attrs, &pred, atoms, path)?;
                record_span(out, SourceSpan::of_syn(arm), &arm_pred, allow_production);
                walk_expr(path, &arm.body, &arm_pred, allow_production, atoms, out)?;
            }
        }
        syn::Expr::Unsafe(u) => {
            walk_stmts(path, &u.block.stmts, &pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_impl_items(
    path: &Path,
    imp: &syn::ItemImpl,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    walk_generics(path, &imp.generics, inherited, allow_production, atoms, out)?;
    for item in &imp.items {
        match item {
            syn::ImplItem::Fn(func) => {
                let pred = attrs_predicate(&func.attrs, inherited, atoms, path)?;
                let pred = if has_test_or_bench(&func.attrs) {
                    pred.and(CfgPred::Atom(super::cfg_pred::ATOM_TEST))
                } else {
                    pred
                };
                record_span(out, SourceSpan::of_syn(func), &pred, allow_production);
                walk_generics(
                    path,
                    &func.sig.generics,
                    &pred,
                    allow_production,
                    atoms,
                    out,
                )?;
                walk_fn_inputs(path, &func.sig.inputs, &pred, allow_production, atoms, out)?;
                walk_stmts(path, &func.block.stmts, &pred, allow_production, atoms, out)?;
            }
            other => {
                let pred = attrs_predicate(impl_item_attrs(other), inherited, atoms, path)?;
                record_span(out, SourceSpan::of_syn(other), &pred, allow_production);
            }
        }
    }
    Ok(())
}

fn walk_foreign(
    path: &Path,
    fm: &syn::ItemForeignMod,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for item in &fm.items {
        let attrs = match item {
            syn::ForeignItem::Fn(f) => f.attrs.as_slice(),
            syn::ForeignItem::Static(s) => s.attrs.as_slice(),
            syn::ForeignItem::Type(t) => t.attrs.as_slice(),
            syn::ForeignItem::Macro(m) => m.attrs.as_slice(),
            _ => &[],
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

fn walk_trait_items(
    path: &Path,
    tr: &syn::ItemTrait,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    walk_generics(path, &tr.generics, inherited, allow_production, atoms, out)?;
    for item in &tr.items {
        let attrs: &[Attribute] = match item {
            syn::TraitItem::Fn(f) => &f.attrs,
            syn::TraitItem::Type(t) => &t.attrs,
            syn::TraitItem::Const(c) => &c.attrs,
            syn::TraitItem::Macro(m) => &m.attrs,
            _ => continue,
        };
        let pred = attrs_predicate(attrs, inherited, atoms, path)?;
        record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

fn walk_variants(
    path: &Path,
    en: &syn::ItemEnum,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for variant in &en.variants {
        let pred = attrs_predicate(&variant.attrs, inherited, atoms, path)?;
        record_span(out, SourceSpan::of_syn(variant), &pred, allow_production);
    }
    Ok(())
}

fn walk_fields(
    path: &Path,
    fields: &syn::Fields,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for field in fields {
        let pred = attrs_predicate(&field.attrs, inherited, atoms, path)?;
        record_span(out, SourceSpan::of_syn(field), &pred, allow_production);
    }
    Ok(())
}

fn push_include(
    from: &Path,
    mac: &syn::Macro,
    pred: &CfgPred,
    kind: IncludeKind,
    out: &mut WalkOutput,
) {
    if let Some(lit) = extract_include_literal_from_macro(mac) {
        let target = resolve_include_path(from, &lit);
        out.includes.push((target, pred.clone(), kind));
    }
}

fn record_span(out: &mut WalkOutput, span: SourceSpan, pred: &CfgPred, allow_production: bool) {
    out.ranges.push(RoleRange {
        span,
        contexts: contexts_for_pred(pred, allow_production),
    });
}
