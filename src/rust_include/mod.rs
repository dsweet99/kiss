use std::path::{Path, PathBuf};

#[must_use]
pub fn is_rust_source_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs") || ext.eq_ignore_ascii_case("inc"))
}

#[must_use]
pub fn include_stem_from_literal(lit: &str) -> String {
    let filename = lit.rsplit(['/', '\\']).next().unwrap_or(lit);
    let stem = filename
        .strip_suffix(".rs")
        .or_else(|| filename.strip_suffix(".inc"))
        .unwrap_or(filename);
    stem.to_string()
}

#[must_use]
pub fn resolve_include_path(includer: &Path, lit: &str) -> PathBuf {
    let path = Path::new(lit);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        includer
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

pub fn canonical_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn extract_include_literal_from_macro(mac: &syn::Macro) -> Option<String> {
    if !mac.path.is_ident("include") {
        return None;
    }
    let lit: syn::LitStr = syn::parse2(mac.tokens.clone()).ok()?;
    Some(lit.value())
}

#[cfg(test)]
mod rust_include_tests {
    use super::*;

    #[test]
    fn rust_include_helpers_resolve_paths() {
        assert!(is_rust_source_path(Path::new("x.rs")));
        assert!(is_rust_source_path(Path::new("x.inc")));
        assert!(!is_rust_source_path(Path::new("x.py")));
        assert_eq!(include_stem_from_literal("dir/child.inc"), "child");
        let resolved = resolve_include_path(Path::new("src/lib.rs"), "child.rs");
        assert!(resolved.to_string_lossy().ends_with("child.rs"));
        let canon = canonical_path(Path::new("."));
        assert!(canon.is_absolute());
        let mac: syn::Macro = syn::parse_quote!(include!("child.rs"));
        assert_eq!(
            extract_include_literal_from_macro(&mac).as_deref(),
            Some("child.rs")
        );
    }
}
