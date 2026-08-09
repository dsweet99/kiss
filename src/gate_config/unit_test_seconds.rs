//! Ordered path-pattern → seconds table for `max_unit_test_seconds`.

use crate::config::ConfigError;
use toml::Value;

/// Default catch-all: every test under 2s.
pub fn default_max_unit_test_seconds() -> Vec<(String, f64)> {
    vec![("*".to_string(), defaults_max())]
}

pub const fn defaults_max() -> f64 {
    crate::defaults::gate::MAX_UNIT_TEST_SECONDS
}

/// First matching pattern wins. `"*"` matches every selector.
pub fn limit_for_selector(rules: &[(String, f64)], selector: &str) -> f64 {
    let path = selector_path(selector);
    for (pattern, secs) in rules {
        if pattern_matches(pattern, path) {
            return *secs;
        }
    }
    // Parser requires a trailing "*"; keep a safe fallback for empty/partial tables.
    defaults_max()
}

pub fn catch_all_limit(rules: &[(String, f64)]) -> Option<f64> {
    rules.iter().rev().find(|(p, _)| p == "*").map(|(_, s)| *s)
}

pub fn validate_rules(rules: &[(String, f64)]) -> Result<(), String> {
    if rules.is_empty() {
        return Err("max_unit_test_seconds must be non-empty".to_string());
    }
    if rules.last().map(|(p, _)| p.as_str()) != Some("*") {
        return Err(
            "max_unit_test_seconds must end with a \"*\" catch-all pattern".to_string(),
        );
    }
    for (pattern, secs) in rules {
        if pattern.is_empty() {
            return Err("max_unit_test_seconds patterns must be non-empty".to_string());
        }
        if !secs.is_finite() || *secs < 0.0 {
            return Err(format!(
                "max_unit_test_seconds values must be finite and nonnegative, got {secs}"
            ));
        }
    }
    Ok(())
}

/// True when duration is at or above the selector's limit (limit 0 ⇒ any run times out).
pub fn exceeds_limit(rules: &[(String, f64)], selector: &str, duration_secs: f64) -> bool {
    let limit = limit_for_selector(rules, selector);
    duration_secs >= limit
}

pub fn parse_max_unit_test_seconds(value: &Value) -> Result<Vec<(String, f64)>, ConfigError> {
    match value {
        Value::Float(f) => scalar_rules(*f),
        Value::Integer(i) => scalar_rules(int_to_f64(*i)),
        Value::Table(table) => parse_table_rules(table),
        Value::Array(items) => parse_array_rules(items),
        other => Err(ConfigError::InvalidValue {
            key: "max_unit_test_seconds".into(),
            message: format!(
                "expected float, table, or array of [pattern, seconds], got {}",
                other.type_str()
            ),
        }),
    }
}

#[allow(dead_code)]
pub fn format_toml_rules(rules: &[(String, f64)]) -> String {
    if rules.len() == 1 && rules[0].0 == "*" {
        return format!("{}", rules[0].1);
    }
    let mut out = String::from("\n");
    for (pattern, secs) in rules {
        out.push_str(&format!("\"{pattern}\" = {secs}\n"));
    }
    // Caller wraps as [gate.max_unit_test_seconds] when multi-entry.
    out
}

pub fn format_nested_toml_table(rules: &[(String, f64)]) -> String {
    let mut out = String::from("[gate.max_unit_test_seconds]\n");
    for (pattern, secs) in rules {
        out.push_str(&format!("\"{pattern}\" = {secs}\n"));
    }
    out
}

fn scalar_rules(secs: f64) -> Result<Vec<(String, f64)>, ConfigError> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(ConfigError::InvalidValue {
            key: "max_unit_test_seconds".into(),
            message: format!("must be a finite nonnegative number, got {secs}"),
        });
    }
    Ok(vec![("*".to_string(), secs)])
}

fn parse_table_rules(table: &toml::Table) -> Result<Vec<(String, f64)>, ConfigError> {
    let mut rules = Vec::with_capacity(table.len());
    for (pattern, value) in table {
        let secs = value
            .as_float()
            .or_else(|| value.as_integer().map(int_to_f64))
            .ok_or_else(|| ConfigError::InvalidValue {
                key: "max_unit_test_seconds".into(),
                message: format!(
                    "pattern \"{pattern}\" expected float seconds, got {}",
                    value.type_str()
                ),
            })?;
        rules.push((pattern.clone(), secs));
    }
    validate_rules(&rules).map_err(|message| ConfigError::InvalidValue {
        key: "max_unit_test_seconds".into(),
        message,
    })?;
    Ok(rules)
}

fn parse_array_rules(items: &[Value]) -> Result<Vec<(String, f64)>, ConfigError> {
    let mut rules = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let Value::Array(pair) = item else {
            return Err(ConfigError::InvalidValue {
                key: "max_unit_test_seconds".into(),
                message: format!("entry {idx} must be [pattern, seconds]"),
            });
        };
        if pair.len() != 2 {
            return Err(ConfigError::InvalidValue {
                key: "max_unit_test_seconds".into(),
                message: format!("entry {idx} must be [pattern, seconds]"),
            });
        }
        let pattern = pair[0].as_str().ok_or_else(|| ConfigError::InvalidValue {
            key: "max_unit_test_seconds".into(),
            message: format!("entry {idx} pattern must be a string"),
        })?;
        let secs = pair[1]
            .as_float()
            .or_else(|| pair[1].as_integer().map(int_to_f64))
            .ok_or_else(|| ConfigError::InvalidValue {
                key: "max_unit_test_seconds".into(),
                message: format!("entry {idx} seconds must be a number"),
            })?;
        rules.push((pattern.to_string(), secs));
    }
    validate_rules(&rules).map_err(|message| ConfigError::InvalidValue {
        key: "max_unit_test_seconds".into(),
        message,
    })?;
    Ok(rules)
}

#[allow(clippy::cast_precision_loss)]
const fn int_to_f64(i: i64) -> f64 {
    i as f64
}

fn selector_path(selector: &str) -> &str {
    selector.split("::").next().unwrap_or(selector)
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    // Trailing `/` means "directory prefix" (e.g. `tests/` ≡ `tests`).
    let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
    let path = path.trim_start_matches("./");
    if pattern.is_empty() {
        return false;
    }
    path == pattern
        || path.starts_with(&format!("{pattern}/"))
        || path.starts_with(&format!("{pattern}::"))
        || path.starts_with(pattern) && {
            // Allow prefix match on file path segment boundary.
            path.as_bytes().get(pattern.len()) == Some(&b'/')
                || path.as_bytes().get(pattern.len()) == Some(&b':')
                || path.len() == pattern.len()
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_match_wins_and_star_required() {
        let rules = vec![
            ("tests/fast".to_string(), 2.0),
            ("tests/slow".to_string(), 60.0),
            ("*".to_string(), 0.0),
        ];
        assert!((limit_for_selector(&rules, "tests/fast/a.py::t") - 2.0).abs() < f64::EPSILON);
        assert!((limit_for_selector(&rules, "tests/slow/b.py::t") - 60.0).abs() < f64::EPSILON);
        assert!((limit_for_selector(&rules, "tests/other/c.py::t") - 0.0).abs() < f64::EPSILON);
        assert!(exceeds_limit(&rules, "tests/other/c.py::t", 0.0));
        assert!(!exceeds_limit(&rules, "tests/fast/a.py::t", 1.9));
        assert!(exceeds_limit(&rules, "tests/fast/a.py::t", 2.0));
    }

    #[test]
    fn parse_scalar_and_table() {
        let scalar = parse_max_unit_test_seconds(&Value::Float(1.5)).unwrap();
        assert_eq!(scalar, vec![("*".to_string(), 1.5)]);
        let mut ordered = toml::Table::new();
        ordered.insert("tests/fast".into(), Value::Integer(2));
        ordered.insert("*".into(), Value::Integer(0));
        let rules = parse_max_unit_test_seconds(&Value::Table(ordered)).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].0, "*");
    }

    #[test]
    fn reject_missing_star() {
        let mut table = toml::Table::new();
        table.insert("tests/fast".into(), Value::Integer(2));
        let err = parse_max_unit_test_seconds(&Value::Table(table)).unwrap_err();
        assert!(err.to_string().contains("catch-all"));
    }

    #[test]
    fn parse_array_and_formatters() {
        let arr = Value::Array(vec![
            Value::Array(vec![Value::String("tests/fast".into()), Value::Integer(2)]),
            Value::Array(vec![Value::String("*".into()), Value::Integer(0)]),
        ]);
        let rules = parse_max_unit_test_seconds(&arr).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(format_nested_toml_table(&rules).contains("tests/fast"));
        assert_eq!(
            format_nested_toml_table(&default_max_unit_test_seconds()),
            "[gate.max_unit_test_seconds]\n\"*\" = 2\n"
        );
        assert_eq!(catch_all_limit(&rules), Some(0.0));
        assert!(validate_rules(&[]).is_err());
        // Empty table falls back to defaults_max inside limit_for_selector.
        assert!((limit_for_selector(&[], "x.py::t") - defaults_max()).abs() < f64::EPSILON);
        assert!(pattern_matches("tests/fast", "tests/fast/a.py"));
        assert!(!pattern_matches("tests/fast", "tests/slow/a.py"));
        assert!(pattern_matches("tests/", "tests/webtester/a.py"));
        assert!(pattern_matches("tests/", "tests/fast/a.py"));
        assert!(!pattern_matches("tests/", "rust/foo.rs"));
        assert_eq!(selector_path("a.py::t"), "a.py");
        let with_slash = vec![
            ("tests/fast".to_string(), 4.0),
            ("tests/".to_string(), 10.0),
            ("*".to_string(), 0.0),
        ];
        assert!((limit_for_selector(&with_slash, "tests/webtester/a.py::t") - 10.0).abs() < f64::EPSILON);
        assert!((limit_for_selector(&with_slash, "tests/fast/a.py::t") - 4.0).abs() < f64::EPSILON);
    }
}
