use super::*;
use crate::config::{ConfigError, apply_lenient_string_list, parse_string_list_key};

pub(super) fn merge_global_lenient(config: &mut GateConfig, global: &toml::Table) {
    if let Some(s) = get_f64(global, "min_similarity") {
        if !(0.0..=1.0).contains(&s) {
            eprintln!("Error: min_similarity must be 0.0-1.0, got {s}");
            return;
        }
        config.min_similarity = s;
    }
    if let Some(v) = get_bool(global, "duplication_enabled") {
        config.duplication_enabled = v;
    }
    if let Some(v) = get_bool(global, "orphan_module_enabled") {
        config.orphan_module_enabled = v;
    }
    if let Some(v) = get_bool(global, "comment_removal_enabled") {
        config.comment_removal_enabled = v;
    }
    apply_lenient_string_list(global, "docs_allowed", "directory names", |v| {
        config.docs_allowed = v;
    });
    apply_lenient_string_list(global, "orphan_allowed", "directory names", |v| {
        config.orphan_allowed = v;
    });
}

pub(super) fn merge_global_strict(
    config: &mut GateConfig,
    global: &toml::Table,
) -> Result<(), ConfigError> {
    if let Some(s) = try_get_f64(global, "min_similarity")? {
        if !(0.0..=1.0).contains(&s) {
            return Err(ConfigError::InvalidValue {
                key: "min_similarity".into(),
                message: format!("must be 0.0-1.0, got {s}"),
            });
        }
        config.min_similarity = s;
    }
    config.duplication_enabled =
        try_get_bool(global, "duplication_enabled", config.duplication_enabled)?;
    config.orphan_module_enabled = try_get_bool(
        global,
        "orphan_module_enabled",
        config.orphan_module_enabled,
    )?;
    config.comment_removal_enabled = try_get_bool(
        global,
        "comment_removal_enabled",
        config.comment_removal_enabled,
    )?;
    if let Some(v) = global.get("docs_allowed") {
        config.docs_allowed = parse_string_list_key(v, "docs_allowed", "directory names")?;
    }
    if let Some(v) = global.get("orphan_allowed") {
        config.orphan_allowed = parse_string_list_key(v, "orphan_allowed", "directory names")?;
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
pub(super) const fn int_to_f64(i: i64) -> f64 {
    i as f64
}

pub(super) fn try_get_f64(table: &toml::Table, key: &str) -> Result<Option<f64>, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    value
        .as_float()
        .or_else(|| value.as_integer().map(int_to_f64))
        .map(Some)
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: format!("expected float, got {}", value.type_str()),
        })
}

pub(super) fn get_bool(table: &toml::Table, key: &str) -> Option<bool> {
    if let Some(v) = table.get(key) {
        if let Some(b) = v.as_bool() {
            return Some(b);
        }
        eprintln!("Warning: Config key '{key}' expected bool");
    }
    None
}

pub(super) fn try_get_bool(
    table: &toml::Table,
    key: &str,
    default: bool,
) -> Result<bool, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(default);
    };
    value.as_bool().ok_or_else(|| ConfigError::InvalidValue {
        key: key.into(),
        message: "expected bool".into(),
    })
}

pub(super) fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(int_to_f64))
        .or_else(|| {
            eprintln!(
                "Warning: Config key '{key}' expected float, got {}",
                value.type_str()
            );
            None
        })
}
