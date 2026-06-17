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
    fn language_and_symbol_parsing_cover_boundary_shapes() {
        assert_eq!(
            detect_language(Path::new("generated.inc")).unwrap(),
            Language::Rust
        );
        assert!(detect_language(Path::new("README")).is_err());

        assert_eq!(
            parse_symbol_shape("plain_name", Language::Python).unwrap(),
            ("plain_name".to_string(), None)
        );
        assert_eq!(
            parse_symbol_shape("Type.member_1", Language::Rust).unwrap(),
            ("Type".to_string(), Some("member_1".to_string()))
        );
        assert!(parse_symbol_shape("Type.1bad", Language::Rust).is_err());
        assert!(parse_symbol_shape("", Language::Python).is_err());
        assert!(!is_valid_identifier("has-dash", Language::Python));
    }
}
