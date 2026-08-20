use super::*;
use crate::config::{ConfigError, apply_lenient_string_list, get_usize, parse_string_list_key};

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
    Ok(())
}

pub(super) fn merge_test_gates_lenient(config: &mut GateConfig, test: &toml::Table) {
    if let Some(t) = get_usize(test, "test_coverage_threshold") {
        if t > 100 {
            eprintln!("Error: test_coverage_threshold must be 0-100, got {t}");
            return;
        }
        config.test_coverage_threshold = t;
    }
    if let Err(msg) = merge_scope_lenient(test, &mut config.test_coverage_scope) {
        eprintln!("Error: {msg}");
    }
    merge_max_unit_test_seconds_lenient(test, &mut config.max_unit_test_seconds);
    match try_get_max_num_tests(test) {
        Ok(Some(n)) => config.max_num_tests = n,
        Ok(None) => {}
        Err(err) => eprintln!("Error: {err}"),
    }
}

pub(super) fn merge_test_gates_strict(
    config: &mut GateConfig,
    test: &toml::Table,
) -> Result<(), ConfigError> {
    if let Some(t) = get_usize(test, "test_coverage_threshold") {
        if t > 100 {
            return Err(ConfigError::InvalidValue {
                key: "test_coverage_threshold".into(),
                message: format!("must be 0-100, got {t}"),
            });
        }
        config.test_coverage_threshold = t;
    }
    if let Some(scope) = try_get_scope(test)? {
        config.test_coverage_scope = scope;
    }
    if let Some(value) = test.get("max_unit_test_seconds") {
        config.max_unit_test_seconds = unit_test_seconds::parse_max_unit_test_seconds(value)?;
    }
    if let Some(n) = try_get_max_num_tests(test)? {
        config.max_num_tests = n;
    }
    Ok(())
}

pub(super) fn try_get_max_num_tests(table: &toml::Table) -> Result<Option<usize>, ConfigError> {
    let Some(value) = table.get("max_num_tests") else {
        return Ok(None);
    };
    let Some(n) = value.as_integer() else {
        return Err(ConfigError::InvalidValue {
            key: "max_num_tests".into(),
            message: format!("expected nonnegative integer, got {}", value.type_str()),
        });
    };
    usize::try_from(n)
        .map(Some)
        .map_err(|_| ConfigError::InvalidValue {
            key: "max_num_tests".into(),
            message: format!("expected nonnegative integer, got {n}"),
        })
}

pub(super) fn merge_max_unit_test_seconds_lenient(
    table: &toml::Table,
    current: &mut Vec<(String, f64)>,
) {
    let Some(value) = table.get("max_unit_test_seconds") else {
        return;
    };
    match unit_test_seconds::parse_max_unit_test_seconds(value) {
        Ok(rules) => *current = rules,
        Err(err) => eprintln!("Error: {err}"),
    }
}

pub(super) fn merge_scope_lenient(
    table: &toml::Table,
    current: &mut TestCoverageScope,
) -> Result<(), String> {
    let Some(value) = table.get("test_coverage_scope") else {
        return Ok(());
    };
    let Some(raw) = value.as_str() else {
        return Err(format!(
            "test_coverage_scope must be \"by_file\" or \"codebase\", got {}",
            value.type_str()
        ));
    };
    match TestCoverageScope::parse(raw) {
        Ok(scope) => {
            *current = scope;
            Ok(())
        }
        Err(message) => Err(format!("test_coverage_scope {message}")),
    }
}

pub(super) fn try_get_scope(table: &toml::Table) -> Result<Option<TestCoverageScope>, ConfigError> {
    let Some(value) = table.get("test_coverage_scope") else {
        return Ok(None);
    };
    let Some(raw) = value.as_str() else {
        return Err(ConfigError::InvalidValue {
            key: "test_coverage_scope".into(),
            message: format!(
                "must be \"by_file\" or \"codebase\", got {}",
                value.type_str()
            ),
        });
    };
    TestCoverageScope::parse(raw)
        .map(Some)
        .map_err(|message| ConfigError::InvalidValue {
            key: "test_coverage_scope".into(),
            message,
        })
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
