//! cfg(active) helpers for Rust coverable-line walking.
pub(super) fn stmt_cfg_active(stmt: &syn::Stmt) -> bool {
    match stmt {
        syn::Stmt::Local(local) => cfg_attrs_active(&local.attrs),
        syn::Stmt::Item(item) => item_cfg_active(item),
        syn::Stmt::Expr(expr, _) => expr_cfg_active(expr),
        syn::Stmt::Macro(stmt_macro) => cfg_attrs_active(&stmt_macro.attrs),
    }
}

pub(super) fn item_cfg_active(item: &syn::Item) -> bool {
    item_cfg_active_a(item).unwrap_or_else(|| item_cfg_active_b(item))
}

fn item_cfg_active_a(item: &syn::Item) -> Option<bool> {
    Some(match item {
        syn::Item::Const(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Enum(item) => cfg_attrs_active(&item.attrs),
        syn::Item::ExternCrate(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Fn(item) => cfg_attrs_active(&item.attrs),
        syn::Item::ForeignMod(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Impl(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Macro(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Mod(item) => cfg_attrs_active(&item.attrs),
        _ => return None,
    })
}

fn item_cfg_active_b(item: &syn::Item) -> bool {
    match item {
        syn::Item::Static(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Struct(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Trait(item) => cfg_attrs_active(&item.attrs),
        syn::Item::TraitAlias(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Type(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Union(item) => cfg_attrs_active(&item.attrs),
        syn::Item::Use(item) => cfg_attrs_active(&item.attrs),
        _ => true,
    }
}

pub(super) fn expr_cfg_active(expr: &syn::Expr) -> bool {
    expr_cfg_active_a(expr)
        .or_else(|| expr_cfg_active_b(expr))
        .or_else(|| expr_cfg_active_c(expr))
        .or_else(|| expr_cfg_active_d(expr))
        .unwrap_or(true)
}

fn expr_cfg_active_a(expr: &syn::Expr) -> Option<bool> {
    Some(match expr {
        syn::Expr::Array(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Assign(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Async(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Await(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Binary(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Block(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Break(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Call(expr) => cfg_attrs_active(&expr.attrs),
        _ => return None,
    })
}

fn expr_cfg_active_b(expr: &syn::Expr) -> Option<bool> {
    Some(match expr {
        syn::Expr::Cast(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Closure(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Const(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Continue(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Field(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::ForLoop(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Group(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::If(expr) => cfg_attrs_active(&expr.attrs),
        _ => return None,
    })
}

fn expr_cfg_active_c(expr: &syn::Expr) -> Option<bool> {
    Some(match expr {
        syn::Expr::Index(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Infer(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Let(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Lit(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Loop(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Macro(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Match(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::MethodCall(expr) => cfg_attrs_active(&expr.attrs),
        _ => return None,
    })
}

fn expr_cfg_active_d(expr: &syn::Expr) -> Option<bool> {
    Some(match expr {
        syn::Expr::Paren(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Path(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Range(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Reference(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Repeat(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Return(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Struct(expr) => cfg_attrs_active(&expr.attrs),
        _ => return expr_cfg_active_e(expr),
    })
}

fn expr_cfg_active_e(expr: &syn::Expr) -> Option<bool> {
    Some(match expr {
        syn::Expr::Try(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::TryBlock(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Tuple(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Unary(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Unsafe(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Verbatim(_) => true,
        syn::Expr::While(expr) => cfg_attrs_active(&expr.attrs),
        syn::Expr::Yield(expr) => cfg_attrs_active(&expr.attrs),
        _ => true,
    })
}

pub(super) fn cfg_attrs_active(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(|attr| {
        if !attr.path().is_ident("cfg") {
            return true;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return true;
        };
        cfg_expr_active(list.tokens.clone()).unwrap_or(true)
    })
}

pub(super) fn cfg_expr_active(tokens: proc_macro2::TokenStream) -> Option<bool> {
    let mut iter = tokens.into_iter();
    let first = iter.next()?;
    match first {
        proc_macro2::TokenTree::Ident(ident) if ident == "unix" => Some(cfg!(unix)),
        proc_macro2::TokenTree::Ident(ident) if ident == "test" => Some(cfg!(test)),
        proc_macro2::TokenTree::Ident(ident) if ident == "not" => {
            let group = cfg_expect_group(&mut iter)?;
            cfg_expr_active(group.stream()).map(|active| !active)
        }
        proc_macro2::TokenTree::Ident(ident) if ident == "any" => {
            let group = cfg_expect_group(&mut iter)?;
            cfg_any_active(split_cfg_group(group.stream()))
        }
        proc_macro2::TokenTree::Ident(ident) if ident == "all" => {
            let group = cfg_expect_group(&mut iter)?;
            cfg_all_active(split_cfg_group(group.stream()))
        }
        proc_macro2::TokenTree::Ident(ident) if ident == "target_os" => cfg_target_os_active(&mut iter),
        _ => None,
    }
}

fn cfg_expect_group(
    iter: &mut impl Iterator<Item = proc_macro2::TokenTree>,
) -> Option<proc_macro2::Group> {
    match iter.next()? {
        proc_macro2::TokenTree::Group(group) => Some(group),
        _ => None,
    }
}

fn cfg_target_os_active(
    iter: &mut impl Iterator<Item = proc_macro2::TokenTree>,
) -> Option<bool> {
    let proc_macro2::TokenTree::Punct(eq) = iter.next()? else {
        return None;
    };
    if eq.as_char() != '=' {
        return None;
    }
    let proc_macro2::TokenTree::Literal(lit) = iter.next()? else {
        return None;
    };
    Some(literal_string_value(&lit) == Some(std::env::consts::OS))
}

pub(super) fn cfg_any_active(parts: Vec<proc_macro2::TokenStream>) -> Option<bool> {
    let mut saw_unknown = false;
    for part in parts {
        match cfg_expr_active(part) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => saw_unknown = true,
        }
    }
    (!saw_unknown).then_some(false)
}

pub(super) fn cfg_all_active(parts: Vec<proc_macro2::TokenStream>) -> Option<bool> {
    let mut saw_unknown = false;
    for part in parts {
        match cfg_expr_active(part) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => saw_unknown = true,
        }
    }
    (!saw_unknown).then_some(true)
}

pub(super) fn split_cfg_group(tokens: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut parts = Vec::new();
    let mut current = proc_macro2::TokenStream::new();
    for token in tokens {
        if matches!(&token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ',') {
            if !current.is_empty() {
                parts.push(current);
                current = proc_macro2::TokenStream::new();
            }
        } else {
            current.extend([token]);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

pub(super) fn literal_string_value(lit: &proc_macro2::Literal) -> Option<&'static str> {
    let value = lit.to_string();
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    Some(match value {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => return None,
    })
}

