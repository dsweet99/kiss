
use std::path::{Path, PathBuf};

use kiss::Language;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedTestTarget {
    pub raw: String,
    pub path: PathBuf,
    pub symbol: Option<String>,
    pub member: Option<String>,
    pub python_nodeid: Option<String>,
    pub language: Language,
}

pub(crate) fn parse_test_target(raw: &str) -> Result<ParsedTestTarget, String> {
    if raw.is_empty() {
        return Err("target must be non-empty".to_string());
    }
    if let Some((path_part, symbol_part)) = raw.split_once("::") {
        if path_part.is_empty() || symbol_part.is_empty() {
            return Err("target path and symbol must both be non-empty".to_string());
        }
        let path = PathBuf::from(path_part);
        let language = detect_test_target_language(&path)?;
        if language == Language::Python && is_python_nodeid_tail(symbol_part) {
            return Ok(ParsedTestTarget {
                raw: raw.to_string(),
                path,
                symbol: None,
                member: None,
                python_nodeid: Some(raw.to_string()),
                language,
            });
        }
        if symbol_part.contains("::") {
            return Err("only one '::' separator is supported in a test target".to_string());
        }
        let (symbol, member) = parse_symbol_shape(symbol_part, language)?;
        Ok(ParsedTestTarget {
            raw: raw.to_string(),
            path,
            symbol: Some(symbol),
            member,
            python_nodeid: None,
            language,
        })
    } else {
        let path = PathBuf::from(raw);
        let language = detect_test_target_language(&path)?;
        Ok(ParsedTestTarget {
            raw: raw.to_string(),
            path,
            symbol: None,
            member: None,
            python_nodeid: None,
            language,
        })
    }
}

fn detect_test_target_language(path: &Path) -> Result<Language, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("py") => Ok(Language::Python),
        Some(ext) if ext.eq_ignore_ascii_case("rs") => Ok(Language::Rust),
        _ => Err("target path must end in .py or .rs".to_string()),
    }
}

fn is_python_nodeid_tail(symbol_part: &str) -> bool {

    symbol_part.contains("::") || symbol_part.contains('[')
}

fn parse_symbol_shape(
    symbol_part: &str,
    language: Language,
) -> Result<(String, Option<String>), String> {
    if let Some((base, member)) = symbol_part.split_once('.') {
        if member.contains('.') {
            return Err("only one member separator is supported in a test target".to_string());
        }
        if !is_ident(base) || !is_ident(member) {
            return Err(format!(
                "invalid {} symbol in target",
                language_label(language)
            ));
        }
        Ok((base.to_string(), Some(member.to_string())))
    } else if !is_ident(symbol_part) {
        Err(format!(
            "invalid {} symbol in target",
            language_label(language)
        ))
    } else {
        Ok((symbol_part.to_string(), None))
    }
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn language_label(language: Language) -> &'static str {
    super::language_label(language)
}
