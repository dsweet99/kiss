use syn::Item;

use super::collect_use_paths;

pub(crate) fn push_include_edges(
    mac: &syn::Macro,
    _mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    if let Some(lit) = crate::rust_include::extract_include_literal_from_macro(mac) {
        include_literals.push(lit);
    }
}

fn extract_imports_from_item(
    item: &Item,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    match item {
        Item::Use(use_item) => collect_use_paths(&use_item.tree, use_roots),
        Item::Macro(item_macro) => {
            push_include_edges(&item_macro.mac, mod_decls, include_literals);
        }
        Item::Mod(mod_item) if mod_item.content.is_none() => {
            mod_decls.push(mod_item.ident.to_string());
        }
        Item::Mod(mod_item) if mod_item.content.is_some() => {
            if let Some((_, items)) = &mod_item.content {
                extract_imports_from_items(items, use_roots, mod_decls, include_literals);
            }
        }
        Item::Fn(fn_item) => {
            extract_imports_from_block(&fn_item.block, use_roots, mod_decls, include_literals);
        }
        Item::Impl(impl_item) => {
            extract_imports_from_impl(impl_item, use_roots, mod_decls, include_literals);
        }
        Item::Trait(trait_item) => {
            extract_imports_from_trait(trait_item, use_roots, mod_decls, include_literals);
        }
        _ => {}
    }
}

fn extract_imports_from_impl(
    impl_item: &syn::ItemImpl,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    for impl_item in &impl_item.items {
        if let syn::ImplItem::Fn(method) = impl_item {
            extract_imports_from_block(&method.block, use_roots, mod_decls, include_literals);
        }
    }
}

fn extract_imports_from_trait(
    trait_item: &syn::ItemTrait,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    for trait_item in &trait_item.items {
        if let syn::TraitItem::Fn(method) = trait_item
            && let Some(block) = &method.default
        {
            extract_imports_from_block(block, use_roots, mod_decls, include_literals);
        }
    }
}

pub(crate) fn extract_imports_from_items(
    items: &[Item],
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    for item in items {
        extract_imports_from_item(item, use_roots, mod_decls, include_literals);
    }
}

pub(crate) fn extract_imports_from_block(
    block: &syn::Block,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => {
                extract_imports_from_items(
                    std::slice::from_ref(item),
                    use_roots,
                    mod_decls,
                    include_literals,
                );
            }
            syn::Stmt::Expr(expr, _) => {
                extract_imports_from_expr(expr, use_roots, mod_decls, include_literals);
            }
            syn::Stmt::Local(syn::Local {
                init: Some(init), ..
            }) => {
                extract_imports_from_expr(&init.expr, use_roots, mod_decls, include_literals);
            }
            _ => {}
        }
    }
}

fn extract_imports_from_loop_expr(
    expr: &syn::Expr,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    match expr {
        syn::Expr::Loop(loop_expr) => {
            extract_imports_from_block(&loop_expr.body, use_roots, mod_decls, include_literals);
        }
        syn::Expr::While(while_expr) => {
            extract_imports_from_block(&while_expr.body, use_roots, mod_decls, include_literals);
        }
        syn::Expr::ForLoop(for_expr) => {
            extract_imports_from_block(&for_expr.body, use_roots, mod_decls, include_literals);
        }
        _ => {}
    }
}

fn extract_imports_from_cond_expr(
    expr: &syn::Expr,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    match expr {
        syn::Expr::If(if_expr) => {
            extract_imports_from_block(
                &if_expr.then_branch,
                use_roots,
                mod_decls,
                include_literals,
            );
            if let Some((_, else_branch)) = &if_expr.else_branch {
                extract_imports_from_expr(else_branch, use_roots, mod_decls, include_literals);
            }
        }
        syn::Expr::Match(match_expr) => {
            for arm in &match_expr.arms {
                extract_imports_from_expr(&arm.body, use_roots, mod_decls, include_literals);
            }
        }
        _ => {}
    }
}

pub(crate) fn extract_imports_from_expr(
    expr: &syn::Expr,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    match expr {
        syn::Expr::Block(block) => {
            extract_imports_from_block(&block.block, use_roots, mod_decls, include_literals);
        }
        syn::Expr::Async(async_block) => {
            extract_imports_from_block(&async_block.block, use_roots, mod_decls, include_literals);
        }
        syn::Expr::Macro(m) => push_include_edges(&m.mac, mod_decls, include_literals),
        syn::Expr::Closure(closure) => {
            if let syn::Expr::Block(block) = &*closure.body {
                extract_imports_from_block(&block.block, use_roots, mod_decls, include_literals);
            }
        }
        syn::Expr::If(_) | syn::Expr::Match(_) => {
            extract_imports_from_cond_expr(expr, use_roots, mod_decls, include_literals);
        }
        syn::Expr::Loop(_) | syn::Expr::While(_) | syn::Expr::ForLoop(_) => {
            extract_imports_from_loop_expr(expr, use_roots, mod_decls, include_literals);
        }
        syn::Expr::Unsafe(unsafe_expr) => {
            extract_imports_from_block(&unsafe_expr.block, use_roots, mod_decls, include_literals);
        }
        _ => {}
    }
}
