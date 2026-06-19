use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::rust_parsing::ParsedRustFile;

fn path_attribute(attrs: &[syn::Attribute]) -> Option<PathBuf> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(meta) = &attr.meta else {
            return None;
        };
        let syn::Expr::Lit(expr) = &meta.value else {
            return None;
        };
        let syn::Lit::Str(lit) = &expr.lit else {
            return None;
        };
        Some(PathBuf::from(lit.value()))
    })
}

fn external_mod_paths(parent: &Path, module: &syn::ItemMod) -> Vec<PathBuf> {
    if module.content.is_some() {
        return Vec::new();
    }
    let parent_dir = parent.parent().unwrap_or_else(|| Path::new(""));
    if let Some(path) = path_attribute(&module.attrs) {
        return vec![crate::rust_include::canonical_path(&parent_dir.join(path))];
    }
    let name = module.ident.to_string();
    vec![
        crate::rust_include::canonical_path(&parent_dir.join(format!("{name}.rs"))),
        crate::rust_include::canonical_path(&parent_dir.join(&name).join("mod.rs")),
    ]
}

pub fn rust_cfg_test_module_paths(parsed_files: &[&ParsedRustFile]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for parsed in parsed_files {
        for item in &parsed.ast.items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            if !super::has_cfg_test_attribute(&module.attrs) {
                continue;
            }
            out.extend(external_mod_paths(&parsed.path, module));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(path: PathBuf, source: &str) -> ParsedRustFile {
        ParsedRustFile {
            path,
            source: source.to_string(),
            ast: syn::parse_file(source).unwrap(),
        }
    }

    #[test]
    fn cfg_test_module_paths_include_path_attribute_targets() {
        let parent = parsed(
            PathBuf::from("/repo/src/lib.rs"),
            "#[cfg(test)] #[path = \"custom_tests.rs\"] mod tests;\n",
        );

        let paths = rust_cfg_test_module_paths(&[&parent]);

        assert!(paths.contains(&PathBuf::from("/repo/src/custom_tests.rs")));
    }

    #[test]
    fn cfg_test_module_paths_include_default_file_and_mod_rs_targets() {
        let parent = parsed(
            PathBuf::from("/repo/src/lib.rs"),
            "#[cfg(test)] mod tests;\n",
        );

        let paths = rust_cfg_test_module_paths(&[&parent]);

        assert!(paths.contains(&PathBuf::from("/repo/src/tests.rs")));
        assert!(paths.contains(&PathBuf::from("/repo/src/tests/mod.rs")));
    }

    #[test]
    fn cfg_test_module_paths_ignore_inline_and_non_test_modules() {
        let parent = parsed(
            PathBuf::from("/repo/src/lib.rs"),
            "mod product;\n#[cfg(test)] mod inline_tests { fn helper() {} }\n",
        );

        let paths = rust_cfg_test_module_paths(&[&parent]);

        assert!(paths.is_empty());
    }
}
