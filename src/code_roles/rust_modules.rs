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

/// Resolve a `mod x;` (no body) declaration the way rustc does.
///
/// `inline_dirs` is the stack of directory segments contributed by every
/// inline `mod a { .. }` block enclosing this declaration in `parent_file`,
/// outermost first, empty when `module` is declared directly at the top
/// level of a file. Each segment is that inline module's own name, or its
/// `#[path]` value when it declared one (see [`inline_mod_segment`]) — this
/// is how rustc resolves `mod a { mod b { mod x; } }` to
/// `<dir of parent>/<a>/<b>/x.rs` (or `.../x/mod.rs`), and how `#[path]` on
/// an inline mod redirects that segment.
pub fn resolve_external_mod(
    parent_file: &Path,
    inline_dirs: &[PathBuf],
    module: &ItemMod,
    inherited: &CfgPred,
    atoms: &mut AtomInterner,
) -> Result<Vec<ModEdge>, RoleBuildError> {
    let name = module.ident.to_string();
    let conventional = conventional_target(parent_file, inline_dirs, &name)?;
    let path_attr_base = path_attr_base_dir(parent_file, inline_dirs);
    let mut edges = Vec::new();
    let mut used_conditional = false;
    for attr in &module.attrs {
        if let Some((pred, path_val)) = conditional_path_attr(attr, atoms, parent_file)? {
            used_conditional = true;
            let target = path_attr_base.join(path_val);
            edges.push(ModEdge {
                target,
                pred: inherited.clone().and(pred),
            });
        }
    }
    if let Some(direct) = direct_path_attr(&module.attrs) {
        let target = path_attr_base.join(direct);
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

/// The directory rustc bases `#[path]` (and `cfg_attr(.., path = ..)`) on for
/// a mod declared at this position: the parent file's own directory when the
/// declaration sits directly in the file (rustc's rule for `path` attributes
/// "not inside inline module blocks"), or the enclosing inline modules'
/// stacked directory otherwise (rustc's rule for `path` attributes "inside
/// inline module blocks").
fn path_attr_base_dir(parent_file: &Path, inline_dirs: &[PathBuf]) -> PathBuf {
    if inline_dirs.is_empty() {
        parent_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    } else {
        inline_stack_dir(parent_file, inline_dirs)
    }
}

/// The directory a plain (no `#[path]`) `mod x;` is looked up in when nested
/// `inline_dirs` deep inside `parent_file`: `child_module_dir(parent_file)`
/// plus every enclosing inline module's own directory segment, in order.
fn inline_stack_dir(parent_file: &Path, inline_dirs: &[PathBuf]) -> PathBuf {
    inline_dirs
        .iter()
        .fold(child_module_dir(parent_file), |dir, seg| dir.join(seg))
}

/// The directory segment an inline `mod a { .. }` block contributes to its
/// own children's lookup directory: its `#[path]` value if it declared one,
/// else its own identifier — mirrors [`declared_mod_path`], which computes
/// the same "what does this mod's path attribute say" question for display
/// purposes.
pub(crate) fn inline_mod_segment(module: &ItemMod) -> PathBuf {
    match declared_mod_path(module) {
        Some(declared) => PathBuf::from(declared),
        None => PathBuf::from(module.ident.to_string()),
    }
}

/// Where a `mod x;` with no `#[path]`/`cfg_attr(.., path = ..)` resolves,
/// dispatching on whether it sits directly in a file (rustc's plain
/// `mod.rs`/non-`mod.rs` convention, handled by [`conventional_paths`],
/// preserved exactly as before this function existed) or inside one or more
/// enclosing inline module blocks (a plain stacked-directory lookup with no
/// file-level fallback heuristics, since inline blocks never introduce a
/// second implicit crate root the way a loose file under `tests/` can).
fn conventional_target(
    parent_file: &Path,
    inline_dirs: &[PathBuf],
    name: &str,
) -> Result<Option<PathBuf>, RoleBuildError> {
    if inline_dirs.is_empty() {
        return conventional_paths(parent_file, &child_module_dir(parent_file), name);
    }
    let dir = inline_stack_dir(parent_file, inline_dirs);
    pick_mod_file(
        dir.join(format!("{name}.rs")),
        dir.join(name).join("mod.rs"),
        name,
    )
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

pub(crate) fn declared_mod_path(module: &ItemMod) -> Option<String> {
    if let Some(direct) = direct_path_attr(&module.attrs) {
        return Some(direct);
    }
    let mut atoms = AtomInterner::new();
    for attr in &module.attrs {
        if let Ok(Some((_, path_val))) = conditional_path_attr(attr, &mut atoms, Path::new("<mod>"))
        {
            return Some(path_val);
        }
    }
    None
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
        let err =
            resolve_external_mod(&lib, &[], &missing, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(err.to_string().contains("missing module"));

        std::fs::write(src.join("present.rs"), "").unwrap();
        let present: ItemMod = syn::parse_str("mod present;").unwrap();
        let edges = resolve_external_mod(&lib, &[], &present, &CfgPred::True, &mut atoms).unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].target.ends_with("present.rs"));

        let nested = src.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("mod.rs"), "").unwrap();
        let nested_mod: ItemMod = syn::parse_str("mod nested;").unwrap();
        let edges =
            resolve_external_mod(&lib, &[], &nested_mod, &CfgPred::True, &mut atoms).unwrap();
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
        let edges = resolve_external_mod(&lib, &[], &pathed, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges[0].target.ends_with("alt.rs"));

        let cfg_mod: ItemMod =
            syn::parse_str("#[cfg_attr(unix, path = \"alt.rs\")] mod cfgfoo;").unwrap();
        let edges = resolve_external_mod(&lib, &[], &cfg_mod, &CfgPred::True, &mut atoms).unwrap();
        assert!(edges.iter().any(|e| e.target.ends_with("alt.rs")));

        let foo = src.join("foo.rs");
        std::fs::write(&foo, "").unwrap();
        std::fs::write(src.join("foo").join("mod.rs"), "").unwrap();
        let nested_foo: ItemMod = syn::parse_str("mod foo;").unwrap();
        let err =
            resolve_external_mod(&foo, &[], &nested_foo, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(
            err.to_string().contains("missing module"),
            "mod foo inside foo.rs looks for foo/foo.rs, not sibling foo.rs or foo/mod.rs"
        );
        let crate_root: ItemMod = syn::parse_str("mod foo;").unwrap();
        let err =
            resolve_external_mod(&lib, &[], &crate_root, &CfgPred::True, &mut atoms).unwrap_err();
        assert!(err.to_string().contains("ambiguous module"));

        let bad: ItemMod = syn::parse_str("#[cfg_attr(unix, path)] mod z;").unwrap();
        let err = resolve_external_mod(&lib, &[], &bad, &CfgPred::True, &mut atoms).unwrap_err();
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
        let edges = resolve_external_mod(&owner, &[], &child, &CfgPred::True, &mut atoms).unwrap();
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
        let edges = resolve_external_mod(&owner, &[], &tests, &CfgPred::True, &mut atoms).unwrap();
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
        let edges = resolve_external_mod(&check, &[], &child, &CfgPred::True, &mut atoms).unwrap();
        assert_eq!(edges[0].target, impl_rs);
    }

    #[test]
    fn inline_mod_in_mod_rs_file_resolves_under_own_directory() {
        // `src/server/mod.rs` containing `mod tests { mod foreign_tenancy; }`
        // must resolve `foreign_tenancy` at `src/server/tests/foreign_tenancy.rs`
        // -- the EG train-3 layout, not `src/tests/foreign_tenancy.rs` (which is
        // what treating `mod tests` as if it were declared at file level gives).
        let tmp = tempfile::tempdir().unwrap();
        let server = tmp.path().join("src").join("server");
        std::fs::create_dir_all(server.join("tests")).unwrap();
        let mod_rs = server.join("mod.rs");
        std::fs::write(&mod_rs, "").unwrap();
        let child = server.join("tests").join("foreign_tenancy.rs");
        std::fs::write(&child, "").unwrap();
        let mut atoms = AtomInterner::new();
        let decl: ItemMod = syn::parse_str("mod foreign_tenancy;").unwrap();
        let edges = resolve_external_mod(
            &mod_rs,
            &[PathBuf::from("tests")],
            &decl,
            &CfgPred::True,
            &mut atoms,
        )
        .unwrap();
        assert_eq!(edges[0].target, child);
    }

    #[test]
    fn inline_mod_in_non_mod_rs_file_resolves_under_own_directory() {
        // `src/server/reasoning_projection.rs` containing
        // `mod tests { mod sweep_clock; }` must resolve `sweep_clock` at
        // `src/server/reasoning_projection/tests/sweep_clock.rs` -- the file's
        // OWN child directory plus the inline segment, not a sibling of the
        // file or a child of the crate directory.
        let tmp = tempfile::tempdir().unwrap();
        let server = tmp.path().join("src").join("server");
        let tests_dir = server.join("reasoning_projection").join("tests");
        std::fs::create_dir_all(&tests_dir).unwrap();
        let file = server.join("reasoning_projection.rs");
        std::fs::write(&file, "").unwrap();
        let child = tests_dir.join("sweep_clock.rs");
        std::fs::write(&child, "").unwrap();
        let mut atoms = AtomInterner::new();
        let decl: ItemMod = syn::parse_str("mod sweep_clock;").unwrap();
        let edges = resolve_external_mod(
            &file,
            &[PathBuf::from("tests")],
            &decl,
            &CfgPred::True,
            &mut atoms,
        )
        .unwrap();
        assert_eq!(edges[0].target, child);
    }

    #[test]
    fn two_levels_of_inline_mods_stack_both_segments() {
        // `mod a { mod b { mod x; } }` in `lib.rs` resolves `x` at
        // `src/a/b/x.rs` (or `src/a/b/x/mod.rs`), never at `src/x.rs`.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("a").join("b")).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "").unwrap();
        let x_mod_rs = src.join("a").join("b").join("x").join("mod.rs");
        std::fs::create_dir_all(x_mod_rs.parent().unwrap()).unwrap();
        std::fs::write(&x_mod_rs, "").unwrap();
        let mut atoms = AtomInterner::new();
        let decl: ItemMod = syn::parse_str("mod x;").unwrap();
        let edges = resolve_external_mod(
            &lib,
            &[PathBuf::from("a"), PathBuf::from("b")],
            &decl,
            &CfgPred::True,
            &mut atoms,
        )
        .unwrap();
        assert_eq!(edges[0].target, x_mod_rs);
    }

    #[test]
    fn missing_and_ambiguous_still_detected_inside_inline_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("inner")).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "").unwrap();
        let mut atoms = AtomInterner::new();

        let missing: ItemMod = syn::parse_str("mod missing;").unwrap();
        let err = resolve_external_mod(
            &lib,
            &[PathBuf::from("inner")],
            &missing,
            &CfgPred::True,
            &mut atoms,
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing module"));

        std::fs::write(src.join("inner").join("dup.rs"), "").unwrap();
        std::fs::create_dir_all(src.join("inner").join("dup")).unwrap();
        std::fs::write(src.join("inner").join("dup").join("mod.rs"), "").unwrap();
        let dup: ItemMod = syn::parse_str("mod dup;").unwrap();
        let err = resolve_external_mod(
            &lib,
            &[PathBuf::from("inner")],
            &dup,
            &CfgPred::True,
            &mut atoms,
        )
        .unwrap_err();
        assert!(err.to_string().contains("ambiguous module"));
    }

    #[test]
    fn path_attr_on_inline_mod_with_body_redirects_descendants() {
        // Mirrors the reference example: a top-level
        // `#[path = "thread_files"] mod thread { #[path = "tls.rs"] mod local_data; }`
        // loads `local_data` from `<dir of file>/thread_files/tls.rs`.
        // `inline_mod_segment` supplies the "thread_files" stack segment for
        // `thread`; this checks the nested `#[path]` on `local_data` resolves
        // relative to that segment, not relative to `thread`'s own name.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("thread_files")).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "").unwrap();
        let tls = src.join("thread_files").join("tls.rs");
        std::fs::write(&tls, "").unwrap();

        let thread: ItemMod =
            syn::parse_str("#[path = \"thread_files\"] mod thread { mod local_data; }").unwrap();
        let segment = inline_mod_segment(&thread);
        assert_eq!(segment, PathBuf::from("thread_files"));

        let mut atoms = AtomInterner::new();
        let local_data: ItemMod = syn::parse_str("#[path = \"tls.rs\"] mod local_data;").unwrap();
        let edges = resolve_external_mod(&lib, &[segment], &local_data, &CfgPred::True, &mut atoms)
            .unwrap();
        assert_eq!(edges[0].target, tls);
    }

    #[test]
    fn inline_mod_segment_falls_back_to_ident_without_path_attr() {
        let module: ItemMod = syn::parse_str("mod tests { mod x; }").unwrap();
        assert_eq!(inline_mod_segment(&module), PathBuf::from("tests"));
    }
}
