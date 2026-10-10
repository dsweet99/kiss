use std::path::Path;
use syn::{Attribute, Item};

use crate::rust_parsing::ParsedRustFile;

#[must_use]
pub fn has_rust_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        if attribute.path().is_ident("test") {
            return true;
        }
        if !attribute.path().is_ident("cfg_attr") {
            return false;
        }
        let mut position = 0_usize;
        let mut enabled_for_test = false;
        let mut injects_test = false;
        if attribute
            .parse_nested_meta(|meta| {
                position += 1;
                if position == 1 {
                    enabled_for_test = meta.path.is_ident("test");
                } else if enabled_for_test && meta.path.is_ident("test") {
                    injects_test = true;
                }
                Ok(())
            })
            .is_err()
        {
            return false;
        }
        injects_test
    })
}

#[must_use]
pub fn is_binary_entry_point(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "main.rs") {
        return true;
    }
    let path_str = path.to_string_lossy();
    path_str.contains("src/bin/") || path_str.contains("src\\bin\\")
}

fn nested_test_module_prefix(prefix: &str, mod_name: &str) -> String {
    if prefix.is_empty() {
        mod_name.to_string()
    } else {
        format!("{prefix}::{mod_name}")
    }
}

fn attrs_active_on_host(attrs: &[Attribute]) -> bool {
    attrs.iter().all(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return true;
        }
        match attribute.parse_args::<syn::Meta>() {
            Ok(meta) => eval_cfg_meta(&meta),
            Err(_) => true,
        }
    })
}

fn eval_cfg_meta(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) if path.is_ident("windows") => cfg!(windows),
        syn::Meta::Path(path) if path.is_ident("unix") => cfg!(unix),
        syn::Meta::List(list) if list.path.is_ident("not") => {
            syn::parse2::<syn::Meta>(list.tokens.clone())
                .map(|inner| !eval_cfg_meta(&inner))
                .unwrap_or(true)
        }
        syn::Meta::List(list) if list.path.is_ident("any") => punctuated_cfg_metas(&list.tokens)
            .into_iter()
            .any(|m| eval_cfg_meta(&m)),
        syn::Meta::List(list) if list.path.is_ident("all") => {
            let metas = punctuated_cfg_metas(&list.tokens);
            !metas.is_empty() && metas.into_iter().all(|m| eval_cfg_meta(&m))
        }
        _ => true,
    }
}

fn punctuated_cfg_metas(tokens: &proc_macro2::TokenStream) -> Vec<syn::Meta> {
    use syn::parse::Parser;
    use syn::punctuated::Punctuated;
    Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        .parse2(tokens.clone())
        .map(|items| items.into_iter().collect())
        .unwrap_or_default()
}

fn collect_test_fn_ids(items: &[Item], prefix: &str, out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if !attrs_active_on_host(&m.attrs) {
                    continue;
                }
                if let Some((_, mod_items)) = &m.content {
                    let mod_prefix = nested_test_module_prefix(prefix, &m.ident.to_string());
                    collect_test_fn_ids(mod_items, &mod_prefix, out);
                }
            }
            Item::Fn(f) if has_rust_test_attribute(&f.attrs) && attrs_active_on_host(&f.attrs) => {
                out.push(prefixed_test_id(prefix, &f.sig.ident.to_string()));
            }
            Item::Impl(item_impl) => {
                if !attrs_active_on_host(&item_impl.attrs) {
                    continue;
                }
                let Some(owner) = impl_owner_name(&item_impl.self_ty) else {
                    continue;
                };
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(method) = impl_item
                        && has_rust_test_attribute(&method.attrs)
                        && attrs_active_on_host(&method.attrs)
                    {
                        out.push(format!("{owner}::{}", method.sig.ident));
                    }
                }
            }
            _ => {}
        }
    }
}

fn prefixed_test_id(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

pub fn impl_owner_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

#[must_use]
pub fn rust_test_functions_in(parsed: &ParsedRustFile) -> Vec<String> {
    let mut out = Vec::new();
    collect_test_fn_ids(&parsed.ast.items, "", &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rust_test_file_naming() {
        let parsed = ParsedRustFile {
            path: Path::new("src/foo_test.rs").to_path_buf(),
            source: "#[test] fn t() {}".to_string(),
            ast: syn::parse_file("#[test] fn t() {}").unwrap(),
        };
        assert_eq!(rust_test_functions_in(&parsed), vec!["t".to_string()]);
    }

    #[test]
    fn binary_entry_point_detection() {
        assert!(is_binary_entry_point(Path::new("src/main.rs")));
        assert!(is_binary_entry_point(Path::new("src/bin/tool.rs")));
        assert!(is_binary_entry_point(Path::new("tests/main.rs")));
        assert!(!is_binary_entry_point(Path::new("src/lib.rs")));
    }

    #[test]
    fn rust_test_functions_in_empty_file() {
        let parsed = ParsedRustFile {
            path: Path::new("src/empty_test.rs").to_path_buf(),
            source: String::new(),
            ast: syn::parse_file("").unwrap(),
        };
        assert!(rust_test_functions_in(&parsed).is_empty());
    }

    #[test]
    fn rust_test_functions_in_finds_tests() {
        let src = r#"
            #[test]
            fn top() {}
            mod nested {
                #[test]
                fn inner() {}
                #[test]
                #[ignore]
                fn skipped() {}
            }
        "#;
        let parsed = ParsedRustFile {
            path: Path::new("src/demo_test.rs").to_path_buf(),
            source: src.to_string(),
            ast: syn::parse_file(src).unwrap(),
        };
        let ids: Vec<_> = rust_test_functions_in(&parsed);
        assert!(ids.iter().any(|id| id == "top"));
        assert!(ids.iter().any(|id| id == "nested::inner"));
        assert!(ids.iter().any(|id| id == "nested::skipped"));
    }

    #[test]
    fn rust_test_functions_in_finds_impl_tests() {
        let src = "struct T;\nimpl T {\n    #[test]\n    fn method() {}\n}\n";
        let parsed = ParsedRustFile {
            path: Path::new("src/impl_test.rs").to_path_buf(),
            source: src.to_string(),
            ast: syn::parse_file(src).unwrap(),
        };
        assert!(
            rust_test_functions_in(&parsed)
                .iter()
                .any(|id| id == "T::method")
        );
    }

    #[test]
    fn rust_test_functions_in_finds_cfg_attr_test() {
        let src = "#[cfg_attr(test, test)]\nfn generated() {}\n";
        let parsed = ParsedRustFile {
            path: Path::new("src/lib.rs").to_path_buf(),
            source: src.to_string(),
            ast: syn::parse_file(src).unwrap(),
        };
        assert_eq!(
            rust_test_functions_in(&parsed),
            vec!["generated".to_string()]
        );
    }

    #[test]
    fn rust_test_functions_in_ignores_non_test_cfg_attr() {
        let src = "#[cfg_attr(test, ignore)]\nfn helper() {}\n";
        let parsed = ParsedRustFile {
            path: Path::new("src/lib.rs").to_path_buf(),
            source: src.to_string(),
            ast: syn::parse_file(src).unwrap(),
        };
        assert!(rust_test_functions_in(&parsed).is_empty());
    }

    #[test]
    fn rust_test_functions_in_skips_inactive_host_cfg() {
        let src = r#"
            #[test]
            fn always() {}
            #[cfg(windows)]
            #[test]
            fn windows_only() {}
            #[cfg(unix)]
            #[test]
            fn unix_only() {}
        "#;
        let parsed = ParsedRustFile {
            path: Path::new("src/cfg_host_test.rs").to_path_buf(),
            source: src.to_string(),
            ast: syn::parse_file(src).unwrap(),
        };
        let ids = rust_test_functions_in(&parsed);
        assert!(ids.iter().any(|id| id == "always"));
        assert_eq!(ids.iter().any(|id| id == "windows_only"), cfg!(windows));
        assert_eq!(ids.iter().any(|id| id == "unix_only"), cfg!(unix));
    }
}
