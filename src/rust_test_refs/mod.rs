//! Rust test-file detection and test-function discovery.
//!
//! Static-reference ("kiss coverage") analysis was removed; runtime coverage is
//! owned exclusively by `kiss cov`. `rust_test_functions_in` remains for the
//! runtime coverage population selector path.

use std::path::Path;
use syn::{Attribute, Item};

use crate::rust_parsing::ParsedRustFile;

fn is_rs_file(path: &Path) -> bool {
    crate::rust_include::is_rust_source_path(path)
}

fn has_test_naming_pattern(path: &Path) -> bool {
    path.file_stem()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            name == "tests"
                || name.ends_with("_test")
                || name.ends_with("_tests")
                || name.ends_with("_integration")
                || name.ends_with("_test_1")
                || name.ends_with("_test_2")
                || name.starts_with("test_")
                || name.starts_with("tests_")
        })
}

fn is_fake_rust_fixture(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.starts_with("fake_"))
}

#[must_use]
pub fn is_rust_test_file(path: &Path) -> bool {
    is_rs_file(path)
        && !is_fake_rust_fixture(path)
        && (has_test_naming_pattern(path) || crate::test_refs::is_in_test_directory(path))
}

fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test"))
}

fn has_ignore_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("ignore"))
}

/// Returns true if the file path is a Rust binary entry point.
///
/// Excludes paths that contain a **normal** path component named exactly `tests`
/// (Cargo’s integration-test tree).
#[must_use]
pub fn is_binary_entry_point(path: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
    {
        return false;
    }
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

fn collect_test_fn_ids(items: &[Item], prefix: &str, out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, mod_items)) = &m.content {
                    let mod_prefix = nested_test_module_prefix(prefix, &m.ident.to_string());
                    collect_test_fn_ids(mod_items, &mod_prefix, out);
                }
            }
            Item::Fn(f) if has_test_attribute(&f.attrs) && !has_ignore_attribute(&f.attrs) => {
                let fn_name = f.sig.ident.to_string();
                let test_id = if prefix.is_empty() {
                    fn_name
                } else {
                    format!("{prefix}::{fn_name}")
                };
                out.push(test_id);
            }
            _ => {}
        }
    }
}

/// Discover `#[test]` function selectors in a parsed Rust file.
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
        assert!(is_rust_test_file(Path::new("src/foo_test.rs")));
        assert!(is_rust_test_file(Path::new("tests/integration.rs")));
        assert!(!is_rust_test_file(Path::new("src/lib.rs")));
        assert!(!is_rust_test_file(Path::new("tests/fake_helper.rs")));
    }

    #[test]
    fn binary_entry_point_detection() {
        assert!(is_binary_entry_point(Path::new("src/main.rs")));
        assert!(is_binary_entry_point(Path::new("src/bin/tool.rs")));
        assert!(!is_binary_entry_point(Path::new("tests/main.rs")));
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
        assert!(!ids.iter().any(|id| id == "nested::skipped"));
    }
}
