use super::trivial_expr::is_delegation_only_block;
use super::{has_cfg_test_attribute, has_test_attribute};
use crate::units::CodeUnitKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::{ImplItem, Item};

use super::executable_calls::collect_executable_call_references_from_test_fns;
use super::references::collect_rust_references;

/// Returns true if the file path is a Rust binary entry point.
///
/// Excludes paths that contain a **normal** path component named exactly `tests` (Cargo’s
/// integration-test tree), not substring matches — so e.g. `legacy_tests/src/main.rs` is still
/// treated as an entry point.
pub(super) fn is_binary_entry_point(path: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
    {
        return false;
    }
    let path_str = path.to_string_lossy();
    if path.file_name().is_some_and(|n| n == "main.rs") {
        return true;
    }
    path_str.contains("src/bin/") || path_str.contains("src\\bin\\")
}

/// Returns true if the function is a trivial binary entry point that only delegates.
/// Such functions are excluded from coverage requirements since they cannot be
/// directly tested (main cannot be called from tests) and contain no real logic.
pub(super) fn is_trivial_binary_main(f: &syn::ItemFn, path: &Path) -> bool {
    if f.sig.ident != "main" {
        return false;
    }
    if !f.sig.inputs.is_empty() {
        return false;
    }
    if !is_binary_entry_point(path) {
        return false;
    }
    is_delegation_only_block(&f.block)
}

#[derive(Debug, Clone)]
pub struct RustCodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub impl_for_type: Option<String>,
}

pub(super) fn collect_rust_definitions(
    ast: &syn::File,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    if is_binary_entry_point(file) {
        return;
    }
    for item in &ast.items {
        collect_definitions_from_item(item, file, defs);
    }
}

pub(crate) fn is_private(name: &str) -> bool {
    name.starts_with('_')
}

pub(super) fn try_add_def(
    defs: &mut Vec<RustCodeDefinition>,
    name: &str,
    kind: CodeUnitKind,
    file: &Path,
    line: usize,
    impl_for_type: Option<String>,
) {
    if !is_private(name) {
        defs.push(RustCodeDefinition {
            name: name.to_string(),
            kind,
            file: file.to_path_buf(),
            line,
            impl_for_type,
        });
    }
}

pub(super) fn extract_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

pub(super) fn collect_impl_methods(
    impl_block: &syn::ItemImpl,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    let is_trait_impl = impl_block.trait_.is_some();
    let impl_type_name = extract_type_name(&impl_block.self_ty);
    for impl_item in &impl_block.items {
        if let ImplItem::Fn(m) = impl_item {
            if has_test_attribute(&m.attrs) {
                continue;
            }
            let (kind, impl_for) = if is_trait_impl {
                (CodeUnitKind::TraitImplMethod, impl_type_name.clone())
            } else {
                (CodeUnitKind::Method, impl_type_name.clone())
            };
            try_add_def(
                defs,
                &m.sig.ident.to_string(),
                kind,
                file,
                m.sig.ident.span().start().line,
                impl_for,
            );
        }
    }
}

pub(super) fn collect_definitions_from_item(
    item: &Item,
    file: &Path,
    defs: &mut Vec<RustCodeDefinition>,
) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) && !is_trivial_binary_main(f, file) => {
            try_add_def(
                defs,
                &f.sig.ident.to_string(),
                CodeUnitKind::Function,
                file,
                f.sig.ident.span().start().line,
                None,
            );
        }
        Item::Struct(s) => try_add_def(
            defs,
            &s.ident.to_string(),
            CodeUnitKind::Class,
            file,
            s.ident.span().start().line,
            None,
        ),
        Item::Enum(e) => try_add_def(
            defs,
            &e.ident.to_string(),
            CodeUnitKind::Class,
            file,
            e.ident.span().start().line,
            None,
        ),
        Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => collect_impl_methods(i, file, defs),
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for i in items {
                    collect_definitions_from_item(i, file, defs);
                }
            }
        }
        _ => {}
    }
}

fn inline_test_items(ast: &syn::File) -> Vec<syn::Item> {
    let mut out = Vec::new();
    for item in &ast.items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    out.extend(items.iter().cloned());
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                out.push(Item::Fn(f.clone()));
            }
            _ => {}
        }
    }
    out
}

pub(super) fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
    let items = inline_test_items(ast);
    if items.is_empty() {
        return;
    }
    collect_rust_references(
        &syn::File {
            shebang: None,
            attrs: vec![],
            items,
        },
        refs,
        &mut HashSet::new(),
    );
}

pub(super) fn collect_inline_test_module_witnesses(
    ast: &syn::File,
    direct_refs: &mut HashSet<String>,
    call_refs: &mut HashSet<String>,
) {
    let items = inline_test_items(ast);
    if items.is_empty() {
        return;
    }
    let file = syn::File {
        shebang: None,
        attrs: vec![],
        items,
    };
    collect_rust_references(&file, direct_refs, &mut HashSet::new());
    collect_executable_call_references_from_test_fns(&file, call_refs, &mut HashSet::new());
}

#[cfg(test)]
mod definitions_coverage {
    use super::super::trivial_expr::{
        is_qualified_or_known_call, is_trivial_expr, is_trivial_stmt, is_well_known_constructor,
    };
    use super::*;

    #[test]
    fn well_known_constructors_recognized() {
        for name in ["Ok", "Err", "Some", "None"] {
            assert!(is_well_known_constructor(name));
        }
        assert!(!is_well_known_constructor("MyType"));
    }

    #[test]
    fn is_delegation_only_block_variants() {
        assert!(is_delegation_only_block(&syn::parse_str("{}").unwrap()));
        assert!(is_delegation_only_block(
            &syn::parse_str("{ crate::run() }").unwrap()
        ));
        assert!(!is_delegation_only_block(
            &syn::parse_str("{ struct Foo; }").unwrap()
        ));
    }

    #[test]
    fn is_trivial_expr_variants() {
        assert!(is_trivial_expr(&syn::parse_str("42").unwrap()));
        assert!(is_trivial_expr(&syn::parse_str("x").unwrap()));
        assert!(is_trivial_expr(&syn::parse_str("lib::run()").unwrap()));
        assert!(!is_trivial_expr(&syn::parse_str("|| {}").unwrap()));
    }

    #[test]
    fn is_trivial_stmt_variants() {
        assert!(is_trivial_stmt(
            &syn::parse_str::<syn::Stmt>("Ok(());").unwrap()
        ));
        let trivial: syn::Block = syn::parse_str("{ let x = 42; }").unwrap();
        assert!(trivial.stmts.iter().all(is_trivial_stmt));
        let non_trivial: syn::Block = syn::parse_str("{ fn inner() {} }").unwrap();
        assert!(!non_trivial.stmts.iter().all(is_trivial_stmt));
    }

    #[test]
    fn is_qualified_or_known_call_variants() {
        assert!(is_qualified_or_known_call(
            &syn::parse_str("module::func()").unwrap()
        ));
        assert!(is_qualified_or_known_call(
            &syn::parse_str("Ok(())").unwrap()
        ));
        assert!(!is_qualified_or_known_call(
            &syn::parse_str("unknown_func()").unwrap()
        ));
    }

    #[test]
    fn try_add_def_public_and_private() {
        let mut defs = Vec::new();
        try_add_def(
            &mut defs,
            "my_func",
            CodeUnitKind::Function,
            Path::new("t.rs"),
            1,
            None,
        );
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "my_func");
        try_add_def(
            &mut defs,
            "_private",
            CodeUnitKind::Function,
            Path::new("t.rs"),
            1,
            None,
        );
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn collect_rust_definitions_on_file() {
        let code = "fn public_fn() {}\nfn _private_fn() {}\nstruct MyStruct;";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut defs = Vec::new();
        collect_rust_definitions(&ast, Path::new("test.rs"), &mut defs);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"public_fn"));
        assert!(names.contains(&"MyStruct"));
        assert!(!names.contains(&"_private_fn"));
    }

    #[test]
    fn collect_test_module_references_finds_refs() {
        let code = r"
            fn production_fn() {}
            #[cfg(test)]
            mod tests {
                use super::*;
                #[test]
                fn test_it() { production_fn(); }
            }
        ";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut refs = HashSet::new();
        collect_test_module_references(&ast, &mut refs);
        assert!(refs.contains("production_fn"));
    }
}
