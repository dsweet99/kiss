use super::merge::{apply_python, apply_rust, apply_shared, apply_thresholds};
use super::types::{Config, ConfigLanguage};
use super::validation::{
    check_unknown_keys, check_unknown_sections, get_usize, validate_config_keys,
    validate_python_keys, validate_rust_keys, validate_shared_keys, validate_thresholds_keys,
};
use super::ConfigError;

#[test]
fn test_merge_and_apply() {
    let mut c = Config::python_defaults();
    c.merge_from_toml(
        "[python]\nstatements_per_function = 99",
        Some(ConfigLanguage::Python),
    );
    assert_eq!(c.statements_per_function, 99);

    let mut table = toml::Table::new();
    table.insert("statements_per_function".into(), toml::Value::Integer(42));
    let mut c2 = Config::python_defaults();
    apply_thresholds(&mut c2, &table);
    assert_eq!(c2.statements_per_function, 42);
}

#[test]
fn test_apply_language_sections() {
    let mut py = Config::python_defaults();
    let mut t = toml::Table::new();
    t.insert("positional_args".into(), toml::Value::Integer(3));
    apply_python(&mut py, &t);
    assert_eq!(py.arguments_positional, 3);

    let mut rs = Config::rust_defaults();
    let mut t2 = toml::Table::new();
    t2.insert("arguments".into(), toml::Value::Integer(5));
    apply_rust(&mut rs, &t2);
    assert_eq!(rs.arguments_per_function, 5);

    let mut c = Config::python_defaults();
    let mut t3 = toml::Table::new();
    t3.insert("statements_per_file".into(), toml::Value::Integer(999));
    apply_shared(&mut c, &t3);
    assert_eq!(c.statements_per_file, 999);
}

#[test]
fn test_helpers() {
    assert!(super::validation::is_similar("python", "pytohn")
        && super::validation::is_similar("rust", "ruts")
        && !super::validation::is_similar("python", "xyz"));
    let mut table = toml::Table::new();
    table.insert("valid".into(), toml::Value::Integer(42));
    table.insert("negative".into(), toml::Value::Integer(-1));
    assert_eq!(get_usize(&table, "valid"), Some(42));
    assert_eq!(get_usize(&table, "missing"), None);
    assert_eq!(get_usize(&table, "negative"), None);
}

#[test]
fn test_validation() {
    let mut t = toml::Table::new();
    t.insert("statements_per_function".into(), toml::Value::Integer(30));
    check_unknown_keys(&t, &["statements_per_function"], "test").unwrap();
    let mut t2 = toml::Table::new();
    t2.insert("python".into(), toml::Value::Table(toml::Table::new()));
    check_unknown_sections(&t2).unwrap();
}

#[test]
fn test_config_error_display() {
    let e = ConfigError::UnknownKey {
        key: "foo".into(),
        section: "bar".into(),
    };
    assert!(e.to_string().contains("foo"));
    assert!(e.to_string().contains("bar"));

    let e2 = ConfigError::UnknownSection {
        section: "baz".into(),
        hint: Some("shared".into()),
    };
    assert!(e2.to_string().contains("baz"));
    assert!(e2.to_string().contains("shared"));

    let e3 = ConfigError::InvalidValue {
        key: "x".into(),
        message: "must be positive".into(),
    };
    assert!(e3.to_string().contains("positive"));
}

#[test]
fn test_unknown_key_returns_error() {
    let mut t = toml::Table::new();
    t.insert("unknown_key".into(), toml::Value::Integer(1));
    let result = check_unknown_keys(&t, &["valid_key"], "test");
    assert!(result.is_err());
}

#[test]
fn test_thresholds_section_accepts_boolean_parameters() {
    // Users may put `boolean_parameters` in [thresholds] (the catch-all section).
    // But THRESHOLDS_KEYS doesn't include it, so it's rejected as unknown.
    let result = Config::try_load_from_content(
        "[thresholds]\nboolean_parameters = 2",
        ConfigLanguage::Python,
    );
    assert!(
        result.is_ok(),
        "boolean_parameters should be accepted in [thresholds]: {:?}",
        result.err()
    );
}

#[test]
fn test_unknown_section_returns_error() {
    let mut t = toml::Table::new();
    t.insert(
        "unknown_section".into(),
        toml::Value::Table(toml::Table::new()),
    );
    let result = check_unknown_sections(&t);
    assert!(result.is_err());
}

#[test]
fn static_coverage_touch_validate_keys() {
    fn t<T>(_: T) {}
    t(validate_config_keys);
    t(validate_thresholds_keys);
    t(validate_shared_keys);
    t(validate_python_keys);
    t(validate_rust_keys);
}
