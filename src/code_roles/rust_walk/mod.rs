use std::path::{Path, PathBuf};

use syn::{Item, ItemMod};

use crate::rust_include::{extract_include_literal_from_macro, resolve_include_path};

use super::cfg_attr::{attrs_predicate, file_inner_predicate};
use super::cfg_pred::{AtomInterner, CfgPred};
use super::cfg_sat::contexts_for_pred;
use super::error::RoleBuildError;
use super::facts::RoleRange;
use super::rust_include_parse::IncludeKind;
use super::rust_modules::{ModEdge, inline_mod_segment, resolve_external_mod};
use super::rust_walk_attrs::item_attrs;
use super::span::SourceSpan;

mod descend;

pub(crate) use descend::walk_stmts;

pub struct WalkOutput {
    pub ranges: Vec<RoleRange>,
    pub mods: Vec<ModEdge>,
    pub includes: Vec<(PathBuf, CfgPred, IncludeKind)>,
}

#[derive(Clone, Copy)]
pub(super) struct WalkCtx<'a> {
    file: &'a Path,
    inline_dirs: &'a [PathBuf],
}

impl<'a> WalkCtx<'a> {
    fn at_file(file: &'a Path) -> Self {
        WalkCtx {
            file,
            inline_dirs: &[],
        }
    }
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
    let ctx = WalkCtx::at_file(path);
    walk_items(&ctx, &ast.items, &pred, allow_production, atoms, &mut out)?;
    Ok(out)
}

fn walk_items(
    ctx: &WalkCtx<'_>,
    items: &[Item],
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for item in items {
        walk_item(ctx, item, inherited, allow_production, atoms, out)?;
    }
    Ok(())
}

pub(super) fn walk_item(
    ctx: &WalkCtx<'_>,
    item: &Item,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    let pred = attrs_predicate(item_attrs(item), inherited, atoms, path)?;
    record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    walk_item_body(ctx, item, &pred, allow_production, atoms, out)
}

fn walk_item_body(
    ctx: &WalkCtx<'_>,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let path = ctx.file;
    match item {
        Item::Mod(module) => walk_mod(ctx, module, pred, allow_production, atoms, out)?,
        Item::Fn(func) => descend::walk_fn(ctx, func, pred, allow_production, atoms, out)?,
        Item::Impl(imp) => descend::walk_impl_items(ctx, imp, pred, allow_production, atoms, out)?,
        Item::Trait(tr) => descend::walk_trait_items(ctx, tr, pred, allow_production, atoms, out)?,
        Item::ForeignMod(fm) => descend::walk_foreign(ctx, fm, pred, allow_production, atoms, out)?,
        Item::Macro(mac) => push_include(path, &mac.mac, pred, IncludeKind::Items, out),
        other => walk_data_item(ctx, other, pred, allow_production, atoms, out)?,
    }
    Ok(())
}

fn walk_data_item(
    ctx: &WalkCtx<'_>,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match item {
        Item::Enum(en) => {
            descend::walk_generics(ctx, &en.generics, pred, allow_production, atoms, out)?;
            descend::walk_variants(ctx, en, pred, allow_production, atoms, out)?;
        }
        Item::Struct(st) => {
            descend::walk_generics(ctx, &st.generics, pred, allow_production, atoms, out)?;
            descend::walk_fields(ctx, &st.fields, pred, allow_production, atoms, out)?;
        }
        Item::Union(un) => {
            descend::walk_generics(ctx, &un.generics, pred, allow_production, atoms, out)?;
            let fields = syn::Fields::Named(un.fields.clone());
            descend::walk_fields(ctx, &fields, pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_mod(
    ctx: &WalkCtx<'_>,
    module: &ItemMod,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    if let Some((_, items)) = &module.content {
        let mut inline_dirs = ctx.inline_dirs.to_vec();
        inline_dirs.push(inline_mod_segment(module));
        let inner = WalkCtx {
            file: ctx.file,
            inline_dirs: &inline_dirs,
        };
        walk_items(&inner, items, pred, allow_production, atoms, out)?;
    } else {
        out.mods.extend(resolve_external_mod(
            ctx.file,
            ctx.inline_dirs,
            module,
            pred,
            atoms,
        )?);
    }
    Ok(())
}

pub(super) fn push_include(
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

pub(super) fn record_span(
    out: &mut WalkOutput,
    span: SourceSpan,
    pred: &CfgPred,
    allow_production: bool,
) {
    out.ranges.push(RoleRange {
        span,
        contexts: contexts_for_pred(pred, allow_production),
    });
}
