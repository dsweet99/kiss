use syn::Item;

use crate::code_roles::SourceSpan;

use super::collect_use_paths;

pub(crate) struct ImportSink<'a> {
    pub use_roots: &'a mut Vec<String>,
    pub mod_decls: &'a mut Vec<String>,
    pub include_literals: &'a mut Vec<String>,
    pub use_spans: &'a mut Vec<(String, String, SourceSpan)>,
    pub mod_spans: &'a mut Vec<(String, String, SourceSpan)>,
    pub include_spans: &'a mut Vec<(String, String, SourceSpan)>,
    pub module_suffix: String,
}

#[cfg(test)]
pub(crate) fn push_include_edges(
    mac: &syn::Macro,
    _mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    if let Some(lit) = crate::rust_include::extract_include_literal_from_macro(mac) {
        include_literals.push(lit);
    }
}

#[cfg(test)]
fn dummy_sink<'a>(
    use_roots: &'a mut Vec<String>,
    mod_decls: &'a mut Vec<String>,
    include_literals: &'a mut Vec<String>,
    use_spans: &'a mut Vec<(String, String, SourceSpan)>,
    mod_spans: &'a mut Vec<(String, String, SourceSpan)>,
    include_spans: &'a mut Vec<(String, String, SourceSpan)>,
) -> ImportSink<'a> {
    ImportSink {
        use_roots,
        mod_decls,
        include_literals,
        use_spans,
        mod_spans,
        include_spans,
        module_suffix: String::new(),
    }
}

fn record_include(mac: &syn::Macro, span: SourceSpan, sink: &mut ImportSink<'_>) {
    if let Some(lit) = crate::rust_include::extract_include_literal_from_macro(mac) {
        sink.include_literals.push(lit.clone());
        sink.include_spans
            .push((sink.module_suffix.clone(), lit, span));
    }
}

fn extract_imports_from_item(item: &Item, sink: &mut ImportSink<'_>) {
    match item {
        Item::Use(use_item) => {
            let start = sink.use_roots.len();
            collect_use_paths(&use_item.tree, sink.use_roots);
            let span = SourceSpan::of_syn(use_item);
            for name in sink.use_roots[start..].iter().cloned() {
                sink.use_spans
                    .push((sink.module_suffix.clone(), name, span));
            }
        }
        Item::Macro(item_macro) => {
            record_include(&item_macro.mac, SourceSpan::of_syn(item_macro), sink);
        }
        Item::Mod(mod_item) => extract_imports_from_mod(mod_item, sink),
        Item::Fn(fn_item) => extract_imports_from_block_skip(&fn_item.block, sink),
        Item::Impl(impl_item) => extract_imports_from_impl(impl_item, sink),
        Item::Trait(trait_item) => extract_imports_from_trait(trait_item, sink),
        _ => {}
    }
}

fn extract_imports_from_mod(mod_item: &syn::ItemMod, sink: &mut ImportSink<'_>) {
    if let Some((_, items)) = &mod_item.content {
        let nested = if sink.module_suffix.is_empty() {
            mod_item.ident.to_string()
        } else {
            format!("{}::{}", sink.module_suffix, mod_item.ident)
        };
        let prev = std::mem::replace(&mut sink.module_suffix, nested);
        extract_imports_from_items_skip(items, sink);
        sink.module_suffix = prev;
    } else {
        sink.mod_decls.push(mod_item.ident.to_string());
        sink.mod_spans.push((
            sink.module_suffix.clone(),
            mod_item.ident.to_string(),
            SourceSpan::of_syn(mod_item),
        ));
    }
}

fn extract_imports_from_impl(impl_item: &syn::ItemImpl, sink: &mut ImportSink<'_>) {
    for impl_item in &impl_item.items {
        if let syn::ImplItem::Fn(method) = impl_item {
            extract_imports_from_block_skip(&method.block, sink);
        }
    }
}

fn extract_imports_from_trait(trait_item: &syn::ItemTrait, sink: &mut ImportSink<'_>) {
    for trait_item in &trait_item.items {
        if let syn::TraitItem::Fn(method) = trait_item
            && let Some(block) = &method.default
        {
            extract_imports_from_block_skip(block, sink);
        }
    }
}

#[cfg(test)]
pub(crate) fn extract_imports_from_items(
    items: &[Item],
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    let mut use_spans = Vec::new();
    let mut mod_spans = Vec::new();
    let mut include_spans = Vec::new();
    extract_imports_from_items_skip(
        items,
        &mut dummy_sink(
            use_roots,
            mod_decls,
            include_literals,
            &mut use_spans,
            &mut mod_spans,
            &mut include_spans,
        ),
    );
}

pub(crate) fn extract_imports_from_items_skip(items: &[Item], sink: &mut ImportSink<'_>) {
    for item in items {
        extract_imports_from_item(item, sink);
    }
}

#[cfg(test)]
pub(crate) fn extract_imports_from_block(
    block: &syn::Block,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    let mut use_spans = Vec::new();
    let mut mod_spans = Vec::new();
    let mut include_spans = Vec::new();
    extract_imports_from_block_skip(
        block,
        &mut dummy_sink(
            use_roots,
            mod_decls,
            include_literals,
            &mut use_spans,
            &mut mod_spans,
            &mut include_spans,
        ),
    );
}

fn extract_imports_from_block_skip(block: &syn::Block, sink: &mut ImportSink<'_>) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => {
                extract_imports_from_items_skip(std::slice::from_ref(item), sink);
            }
            syn::Stmt::Expr(expr, _) => extract_imports_from_expr_sink(expr, sink),
            syn::Stmt::Local(syn::Local {
                init: Some(init), ..
            }) => extract_imports_from_expr_sink(&init.expr, sink),
            _ => {}
        }
    }
}

fn extract_imports_from_loop_expr(expr: &syn::Expr, sink: &mut ImportSink<'_>) {
    match expr {
        syn::Expr::Loop(loop_expr) => extract_imports_from_block_skip(&loop_expr.body, sink),
        syn::Expr::While(while_expr) => extract_imports_from_block_skip(&while_expr.body, sink),
        syn::Expr::ForLoop(for_expr) => extract_imports_from_block_skip(&for_expr.body, sink),
        _ => {}
    }
}

fn extract_imports_from_cond_expr(expr: &syn::Expr, sink: &mut ImportSink<'_>) {
    match expr {
        syn::Expr::If(if_expr) => {
            extract_imports_from_block_skip(&if_expr.then_branch, sink);
            if let Some((_, else_branch)) = &if_expr.else_branch {
                extract_imports_from_expr_sink(else_branch, sink);
            }
        }
        syn::Expr::Match(match_expr) => {
            for arm in &match_expr.arms {
                extract_imports_from_expr_sink(&arm.body, sink);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) fn extract_imports_from_expr(
    expr: &syn::Expr,
    use_roots: &mut Vec<String>,
    mod_decls: &mut Vec<String>,
    include_literals: &mut Vec<String>,
) {
    let mut use_spans = Vec::new();
    let mut mod_spans = Vec::new();
    let mut include_spans = Vec::new();
    extract_imports_from_expr_sink(
        expr,
        &mut dummy_sink(
            use_roots,
            mod_decls,
            include_literals,
            &mut use_spans,
            &mut mod_spans,
            &mut include_spans,
        ),
    );
}

fn extract_imports_from_expr_sink(expr: &syn::Expr, sink: &mut ImportSink<'_>) {
    match expr {
        syn::Expr::Block(block) => extract_imports_from_block_skip(&block.block, sink),
        syn::Expr::Async(async_block) => {
            extract_imports_from_block_skip(&async_block.block, sink);
        }
        syn::Expr::Macro(m) => {
            record_include(&m.mac, SourceSpan::of_syn(m), sink);
        }
        syn::Expr::Closure(closure) => {
            if let syn::Expr::Block(block) = &*closure.body {
                extract_imports_from_block_skip(&block.block, sink);
            }
        }
        syn::Expr::If(_) | syn::Expr::Match(_) => extract_imports_from_cond_expr(expr, sink),
        syn::Expr::Loop(_) | syn::Expr::While(_) | syn::Expr::ForLoop(_) => {
            extract_imports_from_loop_expr(expr, sink);
        }
        syn::Expr::Unsafe(unsafe_expr) => {
            extract_imports_from_block_skip(&unsafe_expr.block, sink);
        }
        _ => {}
    }
}
