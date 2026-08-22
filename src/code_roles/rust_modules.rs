use std::path::{Path, PathBuf};

use syn::{Attribute, ItemMod, Meta};

use super::cfg_parse::{parse_cfg_tokens, take_until_comma};
use super::cfg_pred::{AtomInterner, CfgPred};
use super::error::RoleBuildError;

#[derive(Clone, Debug)]
pub struct ModEdge {
    pub target: PathBuf,
    pub pred: CfgPred,
}

pub fn child_module_dir(parent_file: &Path) -> PathBuf {
    let parent = parent_file.parent().unwrap_or_else(|| Path::new("."));
    let stem = parent_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mod");
    if matches!(stem, "lib" | "main" | "mod") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    }
}

pub fn resolve_external_mod(
    parent_file: &Path,
    module: &ItemMod,
    inherited: &CfgPred,
    atoms: &mut AtomInterner,
) -> Result<Vec<ModEdge>, RoleBuildError> {
    let name = module.ident.to_string();
    let search_dir = child_module_dir(parent_file);
    let conventional = conventional_paths(parent_file, &search_dir, &name)?;
    let mut edges = Vec::new();
    let mut used_conditional = false;
    for attr in &module.attrs {
        if let Some((pred, path_val)) = conditional_path_attr(attr, atoms, parent_file)? {
            used_conditional = true;
            let target = parent_file
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(path_val);
            edges.push(ModEdge {
                target,
                pred: inherited.clone().and(pred),
            });
        }
    }
    if let Some(direct) = direct_path_attr(&module.attrs) {
        let target = parent_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(direct);
        edges.push(ModEdge {
            target,
            pred: inherited.clone(),
        });
        return Ok(edges);
    }
    if used_conditional {
        if let Some(conv) = conventional {
            let mut not_conds = inherited.clone();
            for attr in &module.attrs {
                if let Some((pred, _)) = conditional_path_attr(attr, atoms, parent_file)? {
                    not_conds = not_conds.and(CfgPred::not(pred));
                }
            }
            edges.push(ModEdge {
                target: conv,
                pred: not_conds,
            });
        }
        return Ok(edges);
    }
    let Some(target) = conventional else {
        return Err(RoleBuildError::MissingModule {
            from: parent_file.to_path_buf(),
            name,
        });
    };
    edges.push(ModEdge {
        target,
        pred: inherited.clone(),
    });
    Ok(edges)
}

fn conventional_paths(
    parent_file: &Path,
    search_dir: &Path,
    name: &str,
) -> Result<Option<PathBuf>, RoleBuildError> {
    let sibling_dir = parent_file.parent().unwrap_or_else(|| Path::new("."));
    let sibling = pick_mod_file(
        sibling_dir.join(format!("{name}.rs")),
        sibling_dir.join(name).join("mod.rs"),
        name,
    );
    if search_dir == sibling_dir {
        return sibling;
    }
    let child = pick_mod_file(
        search_dir.join(format!("{name}.rs")),
        search_dir.join(name).join("mod.rs"),
        name,
    )?;
    if child.is_some() {
        return Ok(child);
    }
    let parent_stem = parent_file.file_stem().and_then(|stem| stem.to_str());
    if parent_stem == Some(name) {
        return Ok(None);
    }
    sibling
}

fn pick_mod_file(
    rs: PathBuf,
    nested: PathBuf,
    name: &str,
) -> Result<Option<PathBuf>, RoleBuildError> {
    match (rs.is_file(), nested.is_file()) {
        (true, true) => Err(RoleBuildError::AmbiguousModule {
            name: name.to_string(),
            rs,
            mod_rs: nested,
        }),
        (true, false) => Ok(Some(rs)),
        (false, true) => Ok(Some(nested)),
        (false, false) => Ok(None),
    }
}

fn direct_path_attr(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Some(s.value());
        }
    }
    None
}

fn conditional_path_attr(
    attr: &Attribute,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<Option<(CfgPred, String)>, RoleBuildError> {
    if !attr.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Meta::List(list) = &attr.meta else {
        return Ok(None);
    };
    let mut tokens = list.tokens.clone().into_iter();
    let cond_tokens = take_until_comma(&mut tokens);
    let rest: proc_macro2::TokenStream = tokens.collect();
    if !rest.to_string().contains("path") {
        return Ok(None);
    }
    let pred = parse_cfg_tokens(cond_tokens, atoms, path)?;
    let path_val = path_value_from_tokens(rest)?;
    Ok(Some((pred, path_val)))
}

fn path_value_from_tokens(tokens: proc_macro2::TokenStream) -> Result<String, RoleBuildError> {
    let text = tokens.to_string();
    let Some(start) = text.find('"') else {
        return Err(RoleBuildError::MalformedCfg {
            path: PathBuf::from("<path>"),
            message: "cfg_attr path missing string".into(),
        });
    };
    let rest = &text[start + 1..];
    let Some(end) = rest.find('"') else {
        return Err(RoleBuildError::MalformedCfg {
            path: PathBuf::from("<path>"),
            message: "unterminated path string".into(),
        });
    };
    Ok(rest[..end].to_string())
}

#[cfg(test)]
mod modules_test {
    use super::*;

    #[test]
    fn child_dir_for_lib_and_file_module() {
        assert_eq!(
            child_module_dir(Path::new("src/lib.rs")),
            PathBuf::from("src")
        );
        assert_eq!(
            child_module_dir(Path::new("src/foo.rs")),
            PathBuf::from("src/foo")
        );
        assert_eq!(
            child_module_dir(Path::new("src/foo/mod.rs")),
            PathBuf::from("src/foo")
        );
        assert_eq!(
            child_module_dir(Path::new("src/main.rs")),
            PathBuf::from("src")
        );
    }

    #[test]
    fn resolve_external_mod_conventional_and_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "").unwrap();
        let mut atoms = AtomInterner::new();
        let missing: ItemMod = syn::parse_str("mod missing;").unwrap();
        let err = resolve_external_mod(&lib, &missing, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(err.to_string().contains("missing module"));

        std::fs::write(src.join("present.rs"), "").unwrap();
        let present: ItemMod = syn::parse_str("mod present;").unwrap();
        let edges = resolve_external_mod(&lib, &present, &CfgPred::True, &mut atoms).unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].target.ends_with("present.rs"));

        let nested = src.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("mod.rs"), "").unwrap();
        let nested_mod: ItemMod = syn::parse_str("mod nested;").unwrap();
        let edges = resolve_external_mod(&lib, &nested_mod, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges[0].target.ends_with("mod.rs"));
    }

    #[test]
    fn resolve_external_mod_path_cfg_attr_and_ambiguous() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("foo")).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "").unwrap();
        std::fs::write(src.join("alt.rs"), "").unwrap();
        let mut atoms = AtomInterner::new();
        let pathed: ItemMod = syn::parse_str("#[path = \"alt.rs\"] mod foo;").unwrap();
        let edges = resolve_external_mod(&lib, &pathed, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges[0].target.ends_with("alt.rs"));

        let cfg_mod: ItemMod =
            syn::parse_str("#[cfg_attr(unix, path = \"alt.rs\")] mod cfgfoo;").unwrap();
        let edges = resolve_external_mod(&lib, &cfg_mod, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges.iter().any(|e| e.target.ends_with("alt.rs")));

        let foo = src.join("foo.rs");
        std::fs::write(&foo, "").unwrap();
        std::fs::write(src.join("foo").join("mod.rs"), "").unwrap();
        let nested_foo: ItemMod = syn::parse_str("mod foo;").unwrap();
        let err = resolve_external_mod(&foo, &nested_foo, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(
            err.to_string().contains("missing module"),
            "mod foo inside foo.rs looks for foo/foo.rs, not sibling foo.rs or foo/mod.rs"
        );
        let crate_root: ItemMod = syn::parse_str("mod foo;").unwrap();
        let err = resolve_external_mod(&lib, &crate_root, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(err.to_string().contains("ambiguous module"));

        let bad: ItemMod = syn::parse_str("#[cfg_attr(unix, path)] mod z;").unwrap();
        let err = resolve_external_mod(&lib, &bad, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(err.to_string().contains("malformed cfg"));
    }

    #[test]
    fn named_in_dir_and_non_path_attrs_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("owner")).unwrap();
        let owner = src.join("owner.rs");
        std::fs::write(&owner, "").unwrap();
        std::fs::write(src.join("owner").join("child.rs"), "").unwrap();
        let mut atoms = AtomInterner::new();
        let child: ItemMod = syn::parse_str("#[allow(dead_code)] mod child;").unwrap();
        let edges = resolve_external_mod(&owner, &child, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges[0].target.ends_with("child.rs"));
    }

    #[test]
    fn file_module_prefers_named_child_over_uncle_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("owner")).unwrap();
        let owner = src.join("owner.rs");
        std::fs::write(&owner, "").unwrap();
        std::fs::write(src.join("tests.rs"), "fn uncle() {}\n").unwrap();
        let child = src.join("owner").join("tests.rs");
        std::fs::write(&child, "fn child() {}\n").unwrap();
        let mut atoms = AtomInterner::new();
        let tests: ItemMod = syn::parse_str("#[cfg(test)] mod tests;").unwrap();
        let edges = resolve_external_mod(&owner, &tests, &CfgPred::True, &mut atoms).unwrap();
        assert_eq!(edges[0].target, child);
    }

    #[test]
    fn named_crate_root_resolves_sibling_child_module() {
        let tmp = tempfile::tempdir().unwrap();
        let cases = tmp.path().join("tests").join("cases");
        std::fs::create_dir_all(&cases).unwrap();
        let check = cases.join("check.rs");
        let impl_rs = cases.join("check_impl.rs");
        std::fs::write(&check, "").unwrap();
        std::fs::write(&impl_rs, "").unwrap();
        let mut atoms = AtomInterner::new();
        let child: ItemMod = syn::parse_str("mod check_impl;").unwrap();
        let edges = resolve_external_mod(&check, &child, &CfgPred::True, &mut atoms).unwrap();
        assert_eq!(edges[0].target, impl_rs);
    }
}
