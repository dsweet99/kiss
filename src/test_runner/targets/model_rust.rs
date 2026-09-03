use std::path::Path;

use syn::spanned::Spanned;
use syn::{Item, ItemFn};

use super::model::{DirectTestDef, NamedDefinition, SourceModel};
use kiss::Language;
use kiss::ParsedRustFile;
use kiss::rust_test_refs::{has_rust_test_attribute, impl_owner_name, rust_test_functions_in};

pub(super) fn build_rust_model(
    path: &Path,
    content: String,
    line_count: u32,
) -> Result<SourceModel, String> {
    let ast = syn::parse_file(&content)
        .map_err(|e| format!("failed to parse Rust source {}: {e}", path.display()))?;
    let mut definitions = Vec::new();
    collect_items(&ast.items, "", &mut definitions);
    let parsed = ParsedRustFile {
        path: path.to_path_buf(),
        source: content,
        ast,
    };
    let direct_tests = rust_test_functions_in(&parsed)
        .into_iter()
        .map(|selector| direct_test_from_selector(selector, &definitions))
        .collect();
    Ok(SourceModel {
        path: parsed.path,
        language: Language::Rust,
        direct_tests,
        definitions,
        line_count,
    })
}

fn direct_test_from_selector(selector: String, definitions: &[NamedDefinition]) -> DirectTestDef {
    if let Some(def) = definitions
        .iter()
        .find(|def| def.test_selector.as_deref() == Some(selector.as_str()))
    {
        return DirectTestDef {
            selector,
            name: def.member.clone().unwrap_or_else(|| def.name.clone()),
            owner: def.member.as_ref().map(|_| def.name.clone()),
            start_line: def.start_line,
            end_line: def.end_line,
        };
    }
    let (owner, name) = match selector.rsplit_once("::") {
        Some((owner, name)) => (Some(owner.to_string()), name.to_string()),
        None => (None, selector.clone()),
    };
    DirectTestDef {
        selector,
        name,
        owner,
        start_line: 1,
        end_line: 1,
    }
}

fn collect_items(items: &[Item], module_prefix: &str, definitions: &mut Vec<NamedDefinition>) {
    for item in items {
        match item {
            Item::Fn(func) => push_fn(func, module_prefix, None, definitions),
            Item::Mod(module) => push_mod(module, module_prefix, definitions),
            Item::Struct(item) => push_named(item.ident.to_string(), item, definitions),
            Item::Enum(item) => push_named(item.ident.to_string(), item, definitions),
            Item::Trait(item) => push_trait(item, definitions),
            Item::Impl(item_impl) => push_impl(item_impl, definitions),
            Item::Const(item) => push_named(item.ident.to_string(), item, definitions),
            Item::Static(item) => push_named(item.ident.to_string(), item, definitions),
            _ => {}
        }
    }
}

fn push_mod(module: &syn::ItemMod, module_prefix: &str, definitions: &mut Vec<NamedDefinition>) {
    let name = module.ident.to_string();
    let (start_line, end_line) = span_lines(module);
    definitions.push(NamedDefinition {
        name: name.clone(),
        member: None,
        start_line,
        end_line,
        is_unit_test: false,
        test_selector: None,
    });
    let Some((_, nested)) = &module.content else {
        return;
    };
    let nested_prefix = if module_prefix.is_empty() {
        name
    } else {
        format!("{module_prefix}::{name}")
    };
    collect_items(nested, &nested_prefix, definitions);
}

fn push_trait(item: &syn::ItemTrait, definitions: &mut Vec<NamedDefinition>) {
    push_named(item.ident.to_string(), item, definitions);
    for trait_item in &item.items {
        if let syn::TraitItem::Fn(method) = trait_item {
            let (start_line, end_line) = span_lines(method);
            definitions.push(NamedDefinition {
                name: item.ident.to_string(),
                member: Some(method.sig.ident.to_string()),
                start_line,
                end_line,
                is_unit_test: false,
                test_selector: None,
            });
        }
    }
}

fn push_impl(item_impl: &syn::ItemImpl, definitions: &mut Vec<NamedDefinition>) {
    let Some(owner_name) = impl_owner_name(&item_impl.self_ty) else {
        return;
    };
    let (start_line, end_line) = span_lines(item_impl);
    if !definitions
        .iter()
        .any(|def| def.name == owner_name && def.member.is_none())
    {
        definitions.push(NamedDefinition {
            name: owner_name.clone(),
            member: None,
            start_line,
            end_line,
            is_unit_test: false,
            test_selector: None,
        });
    }
    for impl_item in &item_impl.items {
        if let syn::ImplItem::Fn(method) = impl_item {
            push_method(method, &owner_name, definitions);
        }
    }
}

fn push_named<T: Spanned>(name: String, item: &T, definitions: &mut Vec<NamedDefinition>) {
    let (start_line, end_line) = span_lines(item);
    definitions.push(NamedDefinition {
        name,
        member: None,
        start_line,
        end_line,
        is_unit_test: false,
        test_selector: None,
    });
}

fn push_fn(
    func: &ItemFn,
    module_prefix: &str,
    owner: Option<&str>,
    definitions: &mut Vec<NamedDefinition>,
) {
    let name = func.sig.ident.to_string();
    let (start_line, end_line) = span_lines(func);
    let is_test = has_rust_test_attribute(&func.attrs);
    let selector = is_test.then(|| {
        if module_prefix.is_empty() {
            name.clone()
        } else {
            format!("{module_prefix}::{name}")
        }
    });
    let (def_name, def_member) = match owner {
        Some(owner) => (owner.to_string(), Some(name)),
        None => (name, None),
    };
    definitions.push(NamedDefinition {
        name: def_name,
        member: def_member,
        start_line,
        end_line,
        is_unit_test: is_test,
        test_selector: selector,
    });
}

fn push_method(method: &syn::ImplItemFn, owner: &str, definitions: &mut Vec<NamedDefinition>) {
    let name = method.sig.ident.to_string();
    let (start_line, end_line) = span_lines(method);
    let is_test = has_rust_test_attribute(&method.attrs);
    let selector = is_test.then(|| format!("{owner}::{name}"));
    definitions.push(NamedDefinition {
        name: owner.to_string(),
        member: Some(name),
        start_line,
        end_line,
        is_unit_test: is_test,
        test_selector: selector,
    });
}

fn span_lines<T: Spanned>(item: &T) -> (u32, u32) {
    let span = item.span();
    let start = u32::try_from(span.start().line).unwrap_or(1).max(1);
    let end = u32::try_from(span.end().line).unwrap_or(start).max(start);
    (start, end)
}
