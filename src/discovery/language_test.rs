use super::*;
use std::path::Path;

#[test]
fn language_lookup_helpers_round_trip() {
    for (i, language) in Language::ALL.into_iter().enumerate() {
        assert_eq!(language.index(), i);
        assert_eq!(Language::from_label(language.label()), Some(language));
        assert_eq!(
            Language::from_extension(language.extension()),
            Some(language)
        );
        assert!(language.allowed_by(None));
        assert!(language.allowed_by(Some(language)));
    }
    assert_eq!(Language::from_extension("PY"), Some(Language::Python));
    assert_eq!(Language::from_extension("txt"), None);
    assert_eq!(Language::from_label("go"), None);
    assert!(!Language::Python.allowed_by(Some(Language::Rust)));
}

#[test]
fn language_from_path_and_rust_path() {
    assert_eq!(
        Language::from_path(Path::new("a/b.py")),
        Some(Language::Python)
    );
    assert_eq!(
        Language::from_path(Path::new("src/lib.rs")),
        Some(Language::Rust)
    );
    assert_eq!(Language::from_path(Path::new("README.md")), None);
    assert!(Language::is_rust_path(Path::new("src/lib.rs")));
    assert!(!Language::is_rust_path(Path::new("a/b.py")));
}
