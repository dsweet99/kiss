use super::{has_cfg_test_attribute, has_test_attribute, is_covered_by_qualified_ref, RustCodeDefinition};
use super::references::{CallReferenceVisitor, QualifiedModuleRef, ReferenceVisitor};
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

fn is_covered_for_propagation(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    qualified_refs: &HashSet<QualifiedModuleRef>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) -> bool {
    is_covered_by_qualified_ref(def, qualified_refs)
        || (refs.contains(&def.name)
            && name_files.get(&def.name).is_none_or(|files| files.len() <= 1))
}

fn file_has_covered_production_fn(
    file: &Path,
    items: &[syn::Item],
    definitions: &[RustCodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    test_references: &HashSet<String>,
    qualified_references: &HashSet<QualifiedModuleRef>,
) -> bool {
    items.iter().any(|item| {
        let syn::Item::Fn(f) = item else {
            return false;
        };
        if has_cfg_test_attribute(&f.attrs) || has_test_attribute(&f.attrs) {
            return false;
        }
        let fn_name = f.sig.ident.to_string();
        let Some(def) = definitions
            .iter()
            .find(|d| d.file == file && d.name == fn_name)
        else {
            return false;
        };
        is_covered_for_propagation(def, test_references, qualified_references, name_files)
    })
}

fn propagate_const_and_static_refs(
    items: &[syn::Item],
    test_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
) {
    for item in items {
        let expr = match item {
            syn::Item::Const(c) => Some(&c.expr),
            syn::Item::Static(s) => Some(&s.expr),
            _ => None,
        };
        let Some(expr) = expr else {
            continue;
        };
        ReferenceVisitor {
            refs: test_references,
            qualified: qualified_references,
        }
        .visit_expr(expr);
    }
}

fn propagate_from_items(
    file: &Path,
    items: &[syn::Item],
    definitions: &[RustCodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    test_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
) {
    let propagate_tables = file_has_covered_production_fn(
        file,
        items,
        definitions,
        name_files,
        test_references,
        qualified_references,
    );
    if propagate_tables {
        propagate_const_and_static_refs(items, test_references, qualified_references);
    }
    for item in items {
        match item {
            syn::Item::Fn(f)
                if !has_cfg_test_attribute(&f.attrs) && !has_test_attribute(&f.attrs) =>
            {
                let fn_name = f.sig.ident.to_string();
                let Some(def) = definitions
                    .iter()
                    .find(|d| d.file == file && d.name == fn_name)
                else {
                    continue;
                };
                if is_covered_for_propagation(def, test_references, qualified_references, name_files)
                {
                    ReferenceVisitor {
                        refs: test_references,
                        qualified: qualified_references,
                    }
                    .visit_item_fn(f);
                }
            }
            syn::Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, mod_items)) = &m.content {
                    propagate_from_items(
                        file,
                        mod_items,
                        definitions,
                        name_files,
                        test_references,
                        qualified_references,
                    );
                }
            }
            syn::Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => {
                for impl_item in &i.items {
                    if let syn::ImplItem::Fn(m) = impl_item {
                        let fn_name = m.sig.ident.to_string();
                        let Some(def) = definitions
                            .iter()
                            .find(|d| d.file == file && d.name == fn_name)
                        else {
                            continue;
                        };
                        if is_covered_for_propagation(
                            def,
                            test_references,
                            qualified_references,
                            name_files,
                        ) {
                            ReferenceVisitor {
                                refs: test_references,
                                qualified: qualified_references,
                            }
                            .visit_impl_item_fn(m);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn propagate_transitive_production_refs(
    production_files: &[&ParsedRustFile],
    definitions: &[RustCodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    test_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
) {
    loop {
        let before = test_references.len() + qualified_references.len();
        for parsed in production_files {
            propagate_from_items(
                &parsed.path,
                &parsed.ast.items,
                definitions,
                name_files,
                test_references,
                qualified_references,
            );
        }
        if test_references.len() + qualified_references.len() == before {
            break;
        }
    }
}

fn propagate_call_refs_from_items(
    file: &Path,
    items: &[syn::Item],
    definitions: &[RustCodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    call_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f)
                if !has_cfg_test_attribute(&f.attrs) && !has_test_attribute(&f.attrs) =>
            {
                let fn_name = f.sig.ident.to_string();
                let Some(def) = definitions
                    .iter()
                    .find(|d| d.file == file && d.name == fn_name)
                else {
                    continue;
                };
                if is_covered_for_propagation(def, call_references, qualified_references, name_files)
                {
                    CallReferenceVisitor {
                        refs: call_references,
                        qualified: qualified_references,
                    }
                    .visit_item_fn(f);
                }
            }
            syn::Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, mod_items)) = &m.content {
                    propagate_call_refs_from_items(
                        file,
                        mod_items,
                        definitions,
                        name_files,
                        call_references,
                        qualified_references,
                    );
                }
            }
            syn::Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => {
                for impl_item in &i.items {
                    if let syn::ImplItem::Fn(m) = impl_item {
                        let fn_name = m.sig.ident.to_string();
                        let Some(def) = definitions
                            .iter()
                            .find(|d| d.file == file && d.name == fn_name)
                        else {
                            continue;
                        };
                        if is_covered_for_propagation(
                            def,
                            call_references,
                            qualified_references,
                            name_files,
                        ) {
                            CallReferenceVisitor {
                                refs: call_references,
                                qualified: qualified_references,
                            }
                            .visit_impl_item_fn(m);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn propagate_transitive_production_call_refs(
    production_files: &[&ParsedRustFile],
    definitions: &[RustCodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    call_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
) {
    loop {
        let before = call_references.len() + qualified_references.len();
        for parsed in production_files {
            propagate_call_refs_from_items(
                &parsed.path,
                &parsed.ast.items,
                definitions,
                name_files,
                call_references,
                qualified_references,
            );
        }
        if call_references.len() + qualified_references.len() == before {
            break;
        }
    }
}
