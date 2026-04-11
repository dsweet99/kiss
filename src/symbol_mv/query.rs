//! Query parsing for `kiss mv`.

use std::path::PathBuf;

use crate::Language;

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub raw: String,
    pub path: PathBuf,
    pub symbol: String,
    pub member: Option<String>,
    pub language: Language,
}

impl ParsedQuery {
    pub fn old_name(&self) -> &str {
        self.member.as_deref().unwrap_or(self.symbol.as_str())
    }

    pub const fn language_name(&self) -> &'static str {
        super::language_name(self.language)
    }
}

pub fn parse_mv_query(raw: &str) -> Result<ParsedQuery, String> {
    let (path_part, symbol_part) = raw
        .split_once("::")
        .ok_or_else(|| "source must contain '::' (e.g. path.py::name)".to_string())?;
    if path_part.is_empty() || symbol_part.is_empty() {
        return Err("source path and symbol must both be non-empty".to_string());
    }
    let path = PathBuf::from(path_part);
    let language = crate::symbol_mv_support::detect_language(&path)?;
    let (symbol, member) = crate::symbol_mv_support::parse_symbol_shape(symbol_part, language)?;
    Ok(ParsedQuery {
        raw: raw.to_string(),
        path,
        symbol,
        member,
        language,
    })
}

pub fn validate_new_name(new_name: &str, language: Language) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("target name cannot be empty".to_string());
    }
    if new_name.contains('.') || new_name.contains("::") {
        return Err("target must be a bare identifier".to_string());
    }
    if !crate::symbol_mv_support::is_valid_identifier(new_name, language) {
        return Err(format!(
            "invalid {} identifier '{}'",
            super::language_name(language),
            new_name
        ));
    }
    Ok(())
}
