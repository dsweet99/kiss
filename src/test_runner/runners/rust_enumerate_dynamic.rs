use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn rust_file_needs_dynamic_listing(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    if !source.contains('!') && !source.contains("#[") {
        return false;
    }
    let Ok(file) = syn::parse_file(&source) else {
        return false;
    };
    let local_generators = local_test_generating_macro_names(&file.items);
    items_need_dynamic_listing(&file.items, &local_generators)
}

fn attribute_may_generate_tests(attribute: &syn::Attribute) -> bool {
    let name = attribute
        .path()
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "rstest" | "test_case" | "parameterized" | "proptest" | "quickcheck"
    )
}

fn item_macro_name(item_macro: &syn::ItemMacro) -> String {
    item_macro
        .mac
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

fn tokens_contain_test_attribute(tokens: impl std::fmt::Display) -> bool {
    let compact = tokens.to_string().replace(' ', "");
    compact.contains("#[test]") || compact.contains("#[tokio::test]")
}

fn collect_local_test_generating_macros(items: &[syn::Item], names: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Macro(item_macro)
                if item_macro.mac.path.is_ident("macro_rules")
                    && tokens_contain_test_attribute(&item_macro.mac.tokens) =>
            {
                if let Some(name) = &item_macro.ident {
                    names.insert(name.to_string());
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    collect_local_test_generating_macros(nested, names);
                }
            }
            _ => {}
        }
    }
}

fn local_test_generating_macro_names(items: &[syn::Item]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_local_test_generating_macros(items, &mut names);
    names
}

fn items_need_dynamic_listing(items: &[syn::Item], local_generators: &BTreeSet<String>) -> bool {
    items.iter().any(|item| match item {
        syn::Item::Macro(item_macro) => {
            let name = item_macro_name(item_macro);
            matches!(
                name.as_str(),
                "proptest" | "quickcheck" | "test_suite" | "rstest"
            ) || local_generators.contains(&name)
        }
        syn::Item::Mod(item_mod) => item_mod
            .content
            .as_ref()
            .is_some_and(|(_, nested)| items_need_dynamic_listing(nested, local_generators)),
        syn::Item::Fn(item_fn) => item_fn.attrs.iter().any(attribute_may_generate_tests),
        syn::Item::Impl(item_impl) => item_impl.items.iter().any(|item| {
            matches!(
                item,
                syn::ImplItem::Fn(method)
                    if method.attrs.iter().any(attribute_may_generate_tests)
            )
        }),
        _ => false,
    })
}
