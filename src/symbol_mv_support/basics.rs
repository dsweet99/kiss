use std::path::{Path, PathBuf};

use crate::symbol_mv;
use crate::Language;

pub fn detect_language(path: &Path) -> Result<Language, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("py") => Ok(Language::Python),
        Some("rs") => Ok(Language::Rust),
        _ => Err("source path must end in .py or .rs".to_string()),
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
