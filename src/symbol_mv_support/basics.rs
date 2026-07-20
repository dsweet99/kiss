use std::path::{Path, PathBuf};

use crate::Language;
use crate::symbol_mv;

pub fn detect_language(path: &Path) -> Result<Language, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("py") => Ok(Language::Python),
        Some(ext) if ext.eq_ignore_ascii_case("rs") || ext.eq_ignore_ascii_case("inc") => {
            Ok(Language::Rust)
        }
        _ => Err("source path must end in .py, .rs, or .inc".to_string()),
    }
}

pub fn parse_symbol_shape(
    symbol_part: &str,
    language: Language,
) -> Result<(String, Option<String>), String> {
    if let Some((base, member)) = symbol_part.split_once('.') {
        if member.contains('.') {
            return Err("only one member separator is supported in SOURCE".to_string());
        }
        if !is_valid_identifier(base, language) || !is_valid_identifier(member, language) {
            return Err(format!(
                "invalid {} symbol in source",
                symbol_mv::language_name(language)
            ));
        }
        Ok((base.to_string(), Some(member.to_string())))
    } else if !is_valid_identifier(symbol_part, language) {
        Err(format!(
            "invalid {} symbol in source",
            symbol_mv::language_name(language)
        ))
    } else {
        Ok((symbol_part.to_string(), None))
    }
}

pub fn is_valid_identifier(name: &str, _language: Language) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub fn gather_candidate_files(
    paths: &[String],
    ignore: &[String],
    language: Language,
) -> Vec<PathBuf> {
    let (py_files, rs_files) =
        crate::discovery::gather_files_by_lang(paths, Some(language), ignore);
    match language {
        Language::Python => py_files,
        Language::Rust => rs_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_accepts_supported_extensions_case_insensitively() {
        assert_eq!(
            detect_language(Path::new("app.PY")).unwrap(),
            Language::Python
        );
        assert_eq!(
            detect_language(Path::new("lib.RS")).unwrap(),
            Language::Rust
        );
        assert_eq!(
            detect_language(Path::new("generated.inc")).unwrap(),
            Language::Rust
        );
        assert!(detect_language(Path::new("README.md")).is_err());
    }

    #[test]
    fn parse_symbol_shape_accepts_members_and_rejects_invalid_shapes() {
        assert_eq!(
            parse_symbol_shape("Thing.method", Language::Python).unwrap(),
            ("Thing".to_string(), Some("method".to_string()))
        );
        assert_eq!(
            parse_symbol_shape("function_name", Language::Rust).unwrap(),
            ("function_name".to_string(), None)
        );
        assert!(parse_symbol_shape("A.b.c", Language::Python).is_err());
        assert!(parse_symbol_shape("1bad", Language::Rust).is_err());
    }

    #[test]
    fn valid_identifier_allows_underscores_and_ascii_digits_after_first_char() {
        assert!(is_valid_identifier("_value_1", Language::Python));
        assert!(!is_valid_identifier("1_value", Language::Python));
        assert!(!is_valid_identifier("has-dash", Language::Rust));
    }
}
