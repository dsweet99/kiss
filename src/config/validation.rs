use crate::Language;
use crate::config::error::ConfigError;
use crate::config::keys::{PYTHON_KEYS, RUST_KEYS, SHARED_KEYS, THRESHOLDS_KEYS};

pub(crate) fn check_unknown_keys(
    table: &toml::Table,
    valid: &[&str],
    section: &str,
) -> Result<(), ConfigError> {
    for key in table.keys() {
        if !valid.contains(&key.as_str()) {
            return Err(ConfigError::UnknownKey {
                key: key.clone(),
                section: section.to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn check_unknown_sections(table: &toml::Table) -> Result<(), ConfigError> {
    let valid = known_section_names();
    for key in table.keys() {
        if is_known_section(key) {
            continue;
        }
        let hint = if key == "gate" {
            Some("global".to_string())
        } else {
            valid
                .iter()
                .find(|v| similar(key, v))
                .map(|s| (*s).to_string())
        };
        return Err(ConfigError::UnknownSection {
            section: key.clone(),
            hint,
        });
    }
    Ok(())
}

fn known_section_names() -> Vec<&'static str> {
    let mut names = vec!["shared", "thresholds", "global", "test"];
    names.extend(Language::ALL.iter().map(|language| language.label()));
    names
}

fn is_known_section(key: &str) -> bool {
    matches!(key, "shared" | "thresholds" | "global" | "test")
        || Language::from_label(key).is_some()
}

pub(crate) fn validate_config_keys(
    table: &toml::Table,
    lang: Option<Language>,
) -> Result<(), ConfigError> {
    if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) {
        validate_thresholds_keys(t)?;
    }
    if let Some(t) = table.get("shared").and_then(|v| v.as_table()) {
        validate_shared_keys(t)?;
    }
    for language in Language::ALL {
        if language.allowed_by(lang)
            && let Some(section) = table.get(language.label()).and_then(|v| v.as_table())
        {
            validate_language_keys(language, section)?;
        }
    }
    Ok(())
}

fn validate_language_keys(language: Language, table: &toml::Table) -> Result<(), ConfigError> {
    match language {
        Language::Python => validate_python_keys(table),
        Language::Rust => validate_rust_keys(table),
    }
}

pub(crate) fn validate_thresholds_keys(table: &toml::Table) -> Result<(), ConfigError> {
    check_unknown_keys(table, THRESHOLDS_KEYS, "thresholds")
}

pub(crate) fn validate_shared_keys(table: &toml::Table) -> Result<(), ConfigError> {
    check_unknown_keys(table, SHARED_KEYS, "shared")
}

pub(crate) fn validate_python_keys(table: &toml::Table) -> Result<(), ConfigError> {
    check_unknown_keys(table, PYTHON_KEYS, "python")
}

pub(crate) fn validate_rust_keys(table: &toml::Table) -> Result<(), ConfigError> {
    check_unknown_keys(table, RUST_KEYS, "rust")
}

fn similar(a: &str, b: &str) -> bool {
    if a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    let common = a.chars().filter(|c| b.contains(*c)).count();
    common >= a.len().saturating_sub(2) && common >= b.len().saturating_sub(2)
}

pub(crate) fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    let value = table.get(key)?;
    if let Some(v) = value.as_integer() {
        if v < 0 {
            eprintln!("Warning: Config key '{key}' must be non-negative, got {v}");
            return None;
        }
        return usize::try_from(v).ok();
    }
    eprintln!(
        "Warning: Config key '{key}' expected integer, got {}",
        value.type_str()
    );
    None
}

pub fn is_similar(a: &str, b: &str) -> bool {
    similar(a, b)
}

pub(crate) fn parse_string_list(
    value: &toml::Value,
    empty_label: &str,
) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "expected an array of strings".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item
            .as_str()
            .ok_or_else(|| "expected an array of strings".to_string())?;
        let name = s.trim();
        if name.is_empty() {
            return Err(format!("{empty_label} must be non-empty"));
        }
        out.push(name.to_string());
    }
    Ok(out)
}

pub(crate) fn parse_string_list_key(
    value: &toml::Value,
    key: &str,
    empty_label: &str,
) -> Result<Vec<String>, ConfigError> {
    parse_string_list(value, empty_label).map_err(|message| ConfigError::InvalidValue {
        key: key.into(),
        message,
    })
}

pub(crate) fn apply_lenient_string_list(
    table: &toml::Table,
    key: &str,
    empty_label: &str,
    set: impl FnOnce(Vec<String>),
) {
    let Some(v) = table.get(key) else {
        return;
    };
    match parse_string_list(v, empty_label) {
        Ok(values) => set(values),
        Err(message) => eprintln!("Warning: Config key '{key}' {message}"),
    }
}
