use super::{has_cfg_test_attribute, has_test_attribute};
use crate::units::CodeUnitKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::{ImplItem, Item};

use super::references::{collect_rust_references, ReferenceVisitor};
use syn::visit::Visit;

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
        Item::Fn(f) if !has_test_attribute(&f.attrs) => try_add_def(
            defs,
            &f.sig.ident.to_string(),
            CodeUnitKind::Function,
            file,
            f.sig.ident.span().start().line,
            None,
        ),
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

pub(super) fn collect_test_module_references(ast: &syn::File, refs: &mut HashSet<String>) {
    for item in &ast.items {
        match item {
            Item::Mod(m) if has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    collect_rust_references(
                        &syn::File {
                            shebang: None,
                            attrs: vec![],
                            items: items.clone(),
                        },
                        refs,
                    );
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) => {
                ReferenceVisitor { refs }.visit_item_fn(f);
            }
            _ => {}
        }
    }
}
