use crate::config::error::ConfigError;
use crate::config::keys::{PYTHON_KEYS, RUST_KEYS, SHARED_KEYS, THRESHOLDS_KEYS};
use crate::config::types::ConfigLanguage;

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
    const VALID: &[&str] = &["python", "rust", "shared", "thresholds", "gate"];
    for key in table.keys() {
        if VALID.contains(&key.as_str()) {
            continue;
        }
        let hint = VALID
            .iter()
            .find(|v| similar(key, v))
            .map(|s| (*s).to_string());
        return Err(ConfigError::UnknownSection {
            section: key.clone(),
            hint,
        });
    }
    Ok(())
}

pub(crate) fn validate_config_keys(
    table: &toml::Table,
    lang: Option<ConfigLanguage>,
) -> Result<(), ConfigError> {
    if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) {
        validate_thresholds_keys(t)?;
    }
    if let Some(t) = table.get("shared").and_then(|v| v.as_table()) {
        validate_shared_keys(t)?;
    }
    let check_py = lang.is_none() || matches!(lang, Some(ConfigLanguage::Python));
    let check_rs = lang.is_none() || matches!(lang, Some(ConfigLanguage::Rust));
    if check_py && let Some(t) = table.get("python").and_then(|v| v.as_table()) {
        validate_python_keys(t)?;
    }
    if check_rs && let Some(t) = table.get("rust").and_then(|v| v.as_table()) {
        validate_rust_keys(t)?;
    }
    Ok(())
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
