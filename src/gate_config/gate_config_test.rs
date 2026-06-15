use super::*;
use crate::config::{ConfigError, get_usize};

#[test]
fn test_gate_config_merge_from_toml() {
    let mut gate = GateConfig::default();
    gate.merge_from_toml(
        "[gate]\ntest_coverage_threshold = 50\nmin_similarity = 0.8\nduplication_enabled = false",
    );
    assert_eq!(gate.test_coverage_threshold, 50);
    assert!((gate.min_similarity - 0.8).abs() < 0.01);
    assert!(!gate.duplication_enabled);
}

#[test]
fn test_get_usize() {
    let mut table = toml::Table::new();
    table.insert("valid".into(), toml::Value::Integer(42));
    assert_eq!(get_usize(&table, "valid"), Some(42));
    assert_eq!(get_usize(&table, "missing"), None);
    table.insert("negative".into(), toml::Value::Integer(-1));
    assert_eq!(get_usize(&table, "negative"), None);
}

// === Bug-hunting tests ===

#[test]
fn test_min_similarity_integer_accepted() {
    // TOML treats `min_similarity = 1` as an integer, not float.
    // The config should accept integer values and coerce to float.
    let mut gate = GateConfig::default();
    gate.merge_from_toml("[gate]\nmin_similarity = 1");
    assert!(
        (gate.min_similarity - 1.0).abs() < f64::EPSILON,
        "min_similarity = 1 (integer) should be treated as 1.0 (got {})",
        gate.min_similarity
    );
}

#[test]
fn try_load_from_content_rejects_non_numeric_min_similarity() {
    let err = GateConfig::try_load_from_content("[gate]\nmin_similarity = \"bad\"").unwrap_err();
    assert!(matches!(err, ConfigError::InvalidValue { .. }));
}

#[test]
fn try_load_from_content_rejects_non_bool_gate_flag() {
    let err = GateConfig::try_load_from_content("[gate]\nduplication_enabled = 1").unwrap_err();
    assert!(matches!(err, ConfigError::InvalidValue { .. }));
}

#[test]
fn try_load_from_content_accepts_integer_min_similarity() {
    let gate = GateConfig::try_load_from_content("[gate]\nmin_similarity = 1").unwrap();
    assert!((gate.min_similarity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn merge_from_toml_ignores_non_bool_gate_flag() {
    let mut gate = GateConfig::default();
    gate.merge_from_toml("[gate]\nduplication_enabled = \"yes\"");
    assert!(gate.duplication_enabled);
}

#[test]
fn try_get_f64_parses_float_and_integer() {
    let mut table = toml::Table::new();
    table.insert("f".into(), toml::Value::Float(0.5));
    assert_eq!(try_get_f64(&table, "f").unwrap(), Some(0.5));
    table.insert("i".into(), toml::Value::Integer(2));
    assert_eq!(try_get_f64(&table, "i").unwrap(), Some(2.0));
    assert_eq!(try_get_f64(&table, "missing").unwrap(), None);
    table.insert("bad".into(), toml::Value::String("nope".into()));
    assert!(try_get_f64(&table, "bad").is_err());
}

#[test]
fn try_load_from_content_merges_all_gate_fields() {
    let gate = GateConfig::try_load_from_content(
        "[gate]\ntest_coverage_threshold = 91\nmin_similarity = 1\nduplication_enabled = false\norphan_module_enabled = false",
    )
    .unwrap();
    assert_eq!(gate.test_coverage_threshold, 91);
    assert_eq!(gate.min_similarity, int_to_f64(1));
    assert!(!gate.duplication_enabled);
    assert!(!gate.orphan_module_enabled);
}

#[test]
fn try_load_from_content_rejects_out_of_range_values() {
    let coverage =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_threshold = 101").unwrap_err();
    assert!(matches!(coverage, ConfigError::InvalidValue { .. }));

    let similarity =
        GateConfig::try_load_from_content("[gate]\nmin_similarity = -0.1").unwrap_err();
    assert!(matches!(similarity, ConfigError::InvalidValue { .. }));
}

#[test]
fn get_bool_reads_bool_and_ignores_invalid() {
    let mut table = toml::Table::new();
    table.insert("flag".into(), toml::Value::Boolean(true));
    assert_eq!(get_bool(&table, "flag"), Some(true));
    table.insert("bad".into(), toml::Value::String("nope".into()));
    assert_eq!(get_bool(&table, "bad"), None);
    assert_eq!(get_bool(&table, "missing"), None);
}

#[test]
fn try_get_bool_uses_default_and_rejects_invalid() {
    let table = toml::Table::new();
    assert!(try_get_bool(&table, "flag", true).unwrap());
    let mut table = table;
    table.insert("flag".into(), toml::Value::Boolean(false));
    assert!(!try_get_bool(&table, "flag", true).unwrap());
    table.insert("bad".into(), toml::Value::Integer(1));
    assert!(try_get_bool(&table, "bad", false).is_err());
}

#[test]
fn test_get_f64() {
    let mut table = toml::Table::new();
    table.insert("valid".into(), toml::Value::Float(0.5));
    assert_eq!(get_f64(&table, "valid"), Some(0.5));
    assert_eq!(get_f64(&table, "missing"), None);
}

#[test]
fn witness_try_merge_from_toml_and_int_to_f64() {
    let mut gate = GateConfig::default();
    gate.try_merge_from_toml("[gate]\ntest_coverage_threshold = 75\n")
        .unwrap();
    assert_eq!(gate.test_coverage_threshold, 75);
    assert_eq!(int_to_f64(3), 3.0);
}
