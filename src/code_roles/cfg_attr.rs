use std::path::Path;

use syn::Attribute;

use super::cfg_parse::{parse_cfg_tokens, take_until_comma};
use super::cfg_pred::{AtomInterner, CfgPred};
use super::error::RoleBuildError;

pub const CFG_ATTR_NEST_LIMIT: u32 = 32;

pub fn attrs_predicate(
    attrs: &[Attribute],
    inherited: &CfgPred,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let mut pred = inherited.clone();
    for attr in attrs {
        pred = pred.and(attr_predicate(attr, atoms, path, 0)?);
    }
    Ok(pred)
}

pub fn file_inner_predicate(
    attrs: &[Attribute],
    inherited: &CfgPred,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let mut pred = inherited.clone();
    for attr in attrs {
        if matches!(attr.style, syn::AttrStyle::Inner(_)) {
            pred = pred.and(attr_predicate(attr, atoms, path, 0)?);
        }
    }
    Ok(pred)
}

fn attr_predicate(
    attr: &Attribute,
    atoms: &mut AtomInterner,
    path: &Path,
    depth: u32,
) -> Result<CfgPred, RoleBuildError> {
    if depth > CFG_ATTR_NEST_LIMIT {
        return Err(RoleBuildError::CfgNestingLimit {
            path: path.to_path_buf(),
        });
    }
    if attr.path().is_ident("cfg") {
        return cfg_list_pred(attr, atoms, path);
    }
    if attr.path().is_ident("cfg_attr") {
        return cfg_attr_pred(attr, atoms, path, depth);
    }
    if attr.path().is_ident("test") || attr.path().is_ident("bench") {
        return Ok(CfgPred::Atom(super::cfg_pred::ATOM_TEST));
    }
    Ok(CfgPred::True)
}

fn cfg_list_pred(
    attr: &Attribute,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let syn::Meta::List(list) = &attr.meta else {
        return Ok(CfgPred::True);
    };
    parse_cfg_tokens(list.tokens.clone(), atoms, path)
}

fn cfg_attr_pred(
    attr: &Attribute,
    atoms: &mut AtomInterner,
    path: &Path,
    depth: u32,
) -> Result<CfgPred, RoleBuildError> {
    let syn::Meta::List(list) = &attr.meta else {
        return Err(RoleBuildError::MalformedCfg {
            path: path.to_path_buf(),
            message: "cfg_attr requires a list".into(),
        });
    };
    let mut tokens = list.tokens.clone().into_iter();
    let cond = take_until_comma(&mut tokens);
    let rest: proc_macro2::TokenStream = tokens.collect();
    let p = parse_cfg_tokens(cond, atoms, path)?;
    let inner = parse_cfg_attr_inner(rest, atoms, path, depth + 1)?;
    Ok(CfgPred::not(p).or(inner))
}

fn parse_cfg_attr_inner(
    tokens: proc_macro2::TokenStream,
    atoms: &mut AtomInterner,
    path: &Path,
    depth: u32,
) -> Result<CfgPred, RoleBuildError> {
    if looks_like_cfg(tokens.clone()) {
        let wrapped: Attribute = syn::parse_quote!(#[cfg(#tokens)]);
        return attr_predicate(&wrapped, atoms, path, depth);
    }
    Ok(CfgPred::True)
}

fn looks_like_cfg(tokens: proc_macro2::TokenStream) -> bool {
    match tokens.into_iter().next() {
        Some(proc_macro2::TokenTree::Ident(id)) => id == "cfg",
        _ => false,
    }
}

pub fn has_test_or_bench(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("test") || a.path().is_ident("bench"))
}

#[cfg(test)]
mod attr_test {
    use super::*;
    use crate::code_roles::cfg_sat::sat_with_test;
    use syn::parse_quote;

    #[test]
    fn cfg_attr_encodes_not_p_or_q() {
        let mut atoms = AtomInterner::new();
        let item: syn::Item = parse_quote! {
            #[cfg_attr(feature = "x", cfg(test))]
            fn f() {}
        };
        let syn::Item::Fn(func) = item else { panic!() };
        let pred =
            attrs_predicate(&func.attrs, &CfgPred::True, &mut atoms, Path::new("x.rs")).unwrap();
        assert!(sat_with_test(&pred, false));
        assert!(sat_with_test(&pred, true));
    }

    #[test]
    fn test_attr_forces_test_atom() {
        let mut atoms = AtomInterner::new();
        let item: syn::Item = parse_quote! {
            #[test]
            fn f() {}
        };
        let syn::Item::Fn(func) = item else { panic!() };
        let pred =
            attrs_predicate(&func.attrs, &CfgPred::True, &mut atoms, Path::new("x.rs")).unwrap();
        assert!(!sat_with_test(&pred, false));
        assert!(sat_with_test(&pred, true));
    }
}
