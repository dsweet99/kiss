use std::path::{Path, PathBuf};

use syn::{Attribute, Item, ItemFn, ItemMod, Stmt};

use crate::rust_include::{extract_include_literal_from_macro, resolve_include_path};

use super::cfg_attr::{attrs_predicate, file_inner_predicate, has_test_or_bench};
use super::cfg_pred::{AtomInterner, CfgPred};
use super::cfg_sat::contexts_for_pred;
use super::error::RoleBuildError;
use super::facts::RoleRange;
use super::rust_include_parse::IncludeKind;
use super::rust_modules::{ModEdge, inline_mod_segment, resolve_external_mod};
use super::rust_walk_attrs::{expr_attrs, impl_item_attrs, item_attrs};
use super::span::SourceSpan;

pub struct WalkOutput {
    pub ranges: Vec<RoleRange>,
    pub mods: Vec<ModEdge>,
    pub includes: Vec<(PathBuf, CfgPred, IncludeKind)>,
}

/// Where the walker currently is in the module tree: which file's syntax
/// tree is being walked, and the directory segments contributed by every
/// enclosing inline `mod a { .. }` block (outermost first, empty at the top
/// level of a file). Threaded instead of a bare `&Path` so an external
/// `mod x;` nested inside inline modules can be resolved the way rustc
/// resolves it (see `rust_modules::resolve_external_mod`).
#[derive(Clone, Copy)]
struct WalkCtx<'a> {
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
    ctx: &WalkCtx,
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

fn walk_item(
    ctx: &WalkCtx,
    item: &Item,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = attrs_predicate(item_attrs(item), inherited, atoms, ctx.file)?;
    record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    walk_item_body(ctx, item, &pred, allow_production, atoms, out)
}

fn walk_item_body(
    ctx: &WalkCtx,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match item {
        Item::Mod(module) => walk_mod(ctx, module, pred, allow_production, atoms, out)?,
        Item::Fn(func) => walk_fn(ctx, func, pred, allow_production, atoms, out)?,
        Item::Impl(imp) => walk_impl_items(ctx, imp, pred, allow_production, atoms, out)?,
        Item::Trait(tr) => walk_trait_items(ctx, tr, pred, allow_production, atoms, out)?,
        Item::ForeignMod(fm) => walk_foreign(ctx, fm, pred, allow_production, atoms, out)?,
        Item::Macro(mac) => push_include(ctx.file, &mac.mac, pred, IncludeKind::Items, out),
        other => walk_data_item(ctx, other, pred, allow_production, atoms, out)?,
    }
    Ok(())
}

fn walk_data_item(
    ctx: &WalkCtx,
    item: &Item,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match item {
        Item::Enum(en) => {
            walk_generics(ctx, &en.generics, pred, allow_production, atoms, out)?;
            walk_variants(ctx, en, pred, allow_production, atoms, out)?;
        }
        Item::Struct(st) => {
            walk_generics(ctx, &st.generics, pred, allow_production, atoms, out)?;
            walk_fields(ctx, &st.fields, pred, allow_production, atoms, out)?;
        }
        Item::Union(un) => {
            walk_generics(ctx, &un.generics, pred, allow_production, atoms, out)?;
            let fields = syn::Fields::Named(un.fields.clone());
            walk_fields(ctx, &fields, pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_mod(
    ctx: &WalkCtx,
    module: &ItemMod,
    pred: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    if let Some((_, items)) = &module.content {
        let mut inline_dirs = ctx.inline_dirs.to_vec();
        inline_dirs.push(inline_mod_segment(module));
        let inner_ctx = WalkCtx {
            file: ctx.file,
            inline_dirs: &inline_dirs,
        };
        walk_items(&inner_ctx, items, pred, allow_production, atoms, out)?;
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

fn walk_fn(
    ctx: &WalkCtx,
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
    walk_generics(ctx, &func.sig.generics, &pred, allow_production, atoms, out)?;
    walk_fn_inputs(ctx, &func.sig.inputs, &pred, allow_production, atoms, out)?;
    walk_stmts(ctx, &func.block.stmts, &pred, allow_production, atoms, out)
}

fn walk_fn_inputs(
    ctx: &WalkCtx,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for arg in inputs {
        if let syn::FnArg::Typed(pat) = arg {
            let pred = attrs_predicate(&pat.attrs, inherited, atoms, ctx.file)?;
            record_span(out, SourceSpan::of_syn(pat), &pred, allow_production);
        }
    }
    Ok(())
}

fn walk_generics(
    ctx: &WalkCtx,
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
        let pred = attrs_predicate(attrs, inherited, atoms, ctx.file)?;
        record_span(out, SourceSpan::of_syn(param), &pred, allow_production);
    }
    Ok(())
}

fn walk_stmts(
    ctx: &WalkCtx,
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

/// Entry point for walking a bare statement list read from outside a full
/// file's AST (e.g. content pulled in via `include!`). Always starts at file
/// level: an included statement list is never itself nested inside an inline
/// `mod` block reachable from this call.
pub(crate) fn walk_stmts_at_file(
    path: &Path,
    stmts: &[Stmt],
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    walk_stmts(
        &WalkCtx::at_file(path),
        stmts,
        inherited,
        allow_production,
        atoms,
        out,
    )
}

fn walk_stmt(
    ctx: &WalkCtx,
    stmt: &Stmt,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    match stmt {
        Stmt::Item(item) => walk_item(ctx, item, inherited, allow_production, atoms, out)?,
        Stmt::Local(local) => {
            let pred = attrs_predicate(&local.attrs, inherited, atoms, ctx.file)?;
            record_span(out, SourceSpan::of_syn(local), &pred, allow_production);
        }
        Stmt::Expr(expr, _) => walk_expr(ctx, expr, inherited, allow_production, atoms, out)?,
        Stmt::Macro(mac) => {
            let pred = attrs_predicate(&mac.attrs, inherited, atoms, ctx.file)?;
            record_span(out, SourceSpan::of_syn(mac), &pred, allow_production);
            push_include(ctx.file, &mac.mac, &pred, IncludeKind::Statements, out);
        }
    }
    Ok(())
}

fn walk_expr(
    ctx: &WalkCtx,
    expr: &syn::Expr,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    let pred = attrs_predicate(expr_attrs(expr), inherited, atoms, ctx.file)?;
    record_span(out, SourceSpan::of_syn(expr), &pred, allow_production);
    match expr {
        syn::Expr::Block(block) => {
            walk_stmts(ctx, &block.block.stmts, &pred, allow_production, atoms, out)?;
        }
        syn::Expr::Macro(mac) => {
            push_include(ctx.file, &mac.mac, &pred, IncludeKind::Expr, out);
        }
        syn::Expr::If(if_expr) => {
            walk_stmts(
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
                let arm_pred = attrs_predicate(&arm.attrs, &pred, atoms, ctx.file)?;
                record_span(out, SourceSpan::of_syn(arm), &arm_pred, allow_production);
                walk_expr(ctx, &arm.body, &arm_pred, allow_production, atoms, out)?;
            }
        }
        syn::Expr::Unsafe(u) => {
            walk_stmts(ctx, &u.block.stmts, &pred, allow_production, atoms, out)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_impl_items(
    ctx: &WalkCtx,
    imp: &syn::ItemImpl,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    walk_generics(ctx, &imp.generics, inherited, allow_production, atoms, out)?;
    for item in &imp.items {
        match item {
            syn::ImplItem::Fn(func) => {
                let pred = attrs_predicate(&func.attrs, inherited, atoms, ctx.file)?;
                let pred = if has_test_or_bench(&func.attrs) {
                    pred.and(CfgPred::Atom(super::cfg_pred::ATOM_TEST))
                } else {
                    pred
                };
                record_span(out, SourceSpan::of_syn(func), &pred, allow_production);
                walk_generics(ctx, &func.sig.generics, &pred, allow_production, atoms, out)?;
                walk_fn_inputs(ctx, &func.sig.inputs, &pred, allow_production, atoms, out)?;
                walk_stmts(ctx, &func.block.stmts, &pred, allow_production, atoms, out)?;
            }
            other => {
                let pred = attrs_predicate(impl_item_attrs(other), inherited, atoms, ctx.file)?;
                record_span(out, SourceSpan::of_syn(other), &pred, allow_production);
            }
        }
    }
    Ok(())
}

fn walk_foreign(
    ctx: &WalkCtx,
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
        let pred = attrs_predicate(attrs, inherited, atoms, ctx.file)?;
        record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

fn walk_trait_items(
    ctx: &WalkCtx,
    tr: &syn::ItemTrait,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    walk_generics(ctx, &tr.generics, inherited, allow_production, atoms, out)?;
    for item in &tr.items {
        let attrs: &[Attribute] = match item {
            syn::TraitItem::Fn(f) => &f.attrs,
            syn::TraitItem::Type(t) => &t.attrs,
            syn::TraitItem::Const(c) => &c.attrs,
            syn::TraitItem::Macro(m) => &m.attrs,
            _ => continue,
        };
        let pred = attrs_predicate(attrs, inherited, atoms, ctx.file)?;
        record_span(out, SourceSpan::of_syn(item), &pred, allow_production);
    }
    Ok(())
}

fn walk_variants(
    ctx: &WalkCtx,
    en: &syn::ItemEnum,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for variant in &en.variants {
        let pred = attrs_predicate(&variant.attrs, inherited, atoms, ctx.file)?;
        record_span(out, SourceSpan::of_syn(variant), &pred, allow_production);
    }
    Ok(())
}

fn walk_fields(
    ctx: &WalkCtx,
    fields: &syn::Fields,
    inherited: &CfgPred,
    allow_production: bool,
    atoms: &mut AtomInterner,
    out: &mut WalkOutput,
) -> Result<(), RoleBuildError> {
    for field in fields {
        let pred = attrs_predicate(&field.attrs, inherited, atoms, ctx.file)?;
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

#[cfg(test)]
mod inline_mod_walk_test {
    use super::*;

    #[test]
    fn walk_file_resolves_mod_declared_inside_inline_block() {
        // Regression for the EG train-3 layout: `src/server/mod.rs` declares
        // `mod tests { mod foreign_tenancy; }`. Before this fix, `walk_mod`
        // passed the same top-level `path` down through nested inline mods,
        // so the child was looked for at `src/foreign_tenancy.rs` and
        // `resolve_external_mod` returned `MissingModule`.
        let tmp = tempfile::tempdir().unwrap();
        let server = tmp.path().join("src").join("server");
        std::fs::create_dir_all(server.join("tests")).unwrap();
        let mod_rs = server.join("mod.rs");
        std::fs::write(
            &mod_rs,
            "#[cfg(test)]\nmod tests {\n    mod foreign_tenancy;\n}\n",
        )
        .unwrap();
        std::fs::write(server.join("tests").join("foreign_tenancy.rs"), "").unwrap();
        let ast = syn::parse_file(&std::fs::read_to_string(&mod_rs).unwrap()).unwrap();
        let mut atoms = AtomInterner::new();
        let out = walk_file(&mod_rs, &ast, &CfgPred::True, true, &mut atoms).unwrap();
        assert_eq!(out.mods.len(), 1);
        assert_eq!(
            out.mods[0].target,
            server.join("tests").join("foreign_tenancy.rs")
        );
    }

    #[test]
    fn walk_file_resolves_mod_declared_inside_inline_block_non_mod_rs() {
        // Regression for the EG train-3 layout: a non-`mod.rs` file
        // `reasoning_projection.rs` declares `mod tests { mod sweep_clock; }`,
        // which must resolve under
        // `reasoning_projection/tests/sweep_clock.rs`.
        let tmp = tempfile::tempdir().unwrap();
        let server = tmp.path().join("src").join("server");
        let tests_dir = server.join("reasoning_projection").join("tests");
        std::fs::create_dir_all(&tests_dir).unwrap();
        let file = server.join("reasoning_projection.rs");
        std::fs::write(
            &file,
            "#[cfg(test)]\nmod tests {\n    #[cfg(feature = \"redb\")]\n    mod sweep_clock;\n}\n",
        )
        .unwrap();
        std::fs::write(tests_dir.join("sweep_clock.rs"), "").unwrap();
        let ast = syn::parse_file(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let mut atoms = AtomInterner::new();
        let out = walk_file(&file, &ast, &CfgPred::True, true, &mut atoms).unwrap();
        assert_eq!(out.mods.len(), 1);
        assert_eq!(out.mods[0].target, tests_dir.join("sweep_clock.rs"));
    }
}
