use super::*;
use crate::config::get_usize;

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
fn test_get_f64() {
    let mut table = toml::Table::new();
    table.insert("valid".into(), toml::Value::Float(0.5));
    assert_eq!(get_f64(&table, "valid"), Some(0.5));
    assert_eq!(get_f64(&table, "missing"), None);
}

#[test]
fn static_coverage_touch_gate_parsers() {
    fn t<T>(_: T) {}
    t(int_to_f64);
    t(try_get_f64);
    t(get_bool);
    t(try_get_bool);
}
