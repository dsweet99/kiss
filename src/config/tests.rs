use super::ConfigError;
use super::merge::{apply_python, apply_rust, apply_shared, apply_thresholds};
use super::types::{Config, ConfigLanguage};
use super::validation::{
    check_unknown_keys, check_unknown_sections, get_usize, validate_config_keys,
    validate_python_keys, validate_rust_keys, validate_shared_keys, validate_thresholds_keys,
};

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
    assert_eq!(rs.arguments_positional, 5);

    let mut c = Config::python_defaults();
    let mut t3 = toml::Table::new();
    t3.insert("statements_per_file".into(), toml::Value::Integer(999));
    apply_shared(&mut c, &t3);
    assert_eq!(c.statements_per_file, 999);
}

#[test]
fn apply_section_helpers_ignore_unknown_keys_without_mutating() {
    let mut table = toml::Table::new();
    table.insert("unknown".into(), toml::Value::Integer(1));

    let mut thresholds = Config::python_defaults();
    let before = thresholds.statements_per_function;
    apply_thresholds(&mut thresholds, &table);
    assert_eq!(thresholds.statements_per_function, before);

    let mut shared = Config::python_defaults();
    let before = shared.statements_per_file;
    apply_shared(&mut shared, &table);
    assert_eq!(shared.statements_per_file, before);

    let mut py = Config::python_defaults();
    let before = py.arguments_positional;
    apply_python(&mut py, &table);
    assert_eq!(py.arguments_positional, before);

    let mut rs = Config::rust_defaults();
    let before = rs.arguments_positional;
    apply_rust(&mut rs, &table);
    assert_eq!(rs.arguments_positional, before);
}

#[test]
fn test_helpers() {
    assert!(
        super::validation::is_similar("python", "pytohn")
            && super::validation::is_similar("rust", "ruts")
            && !super::validation::is_similar("python", "xyz")
    );
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
fn validate_section_keys_accept_known_keys() {
    let mut thresholds = toml::Table::new();
    thresholds.insert("statements_per_function".into(), toml::Value::Integer(30));
    validate_thresholds_keys(&thresholds).unwrap();

    let mut shared = toml::Table::new();
    shared.insert("statements_per_file".into(), toml::Value::Integer(100));
    validate_shared_keys(&shared).unwrap();

    let mut python = toml::Table::new();
    python.insert("positional_args".into(), toml::Value::Integer(5));
    validate_python_keys(&python).unwrap();

    let mut rust = toml::Table::new();
    rust.insert("arguments".into(), toml::Value::Integer(5));
    validate_rust_keys(&rust).unwrap();

    let mut root = toml::Table::new();
    root.insert("thresholds".into(), toml::Value::Table(thresholds));
    root.insert("shared".into(), toml::Value::Table(shared));
    root.insert("python".into(), toml::Value::Table(python));
    root.insert("rust".into(), toml::Value::Table(rust));
    validate_config_keys(&root, None).unwrap();
    validate_config_keys(&root, Some(ConfigLanguage::Python)).unwrap();
    validate_config_keys(&root, Some(ConfigLanguage::Rust)).unwrap();
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
fn load_and_load_for_language_with_override_apply_toml() {
    let loaded = Config::load();
    assert!(loaded.statements_per_function > 0);

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[python]\nstatements_per_function = 61\n").unwrap();
    let overridden = Config::load_for_language_with_override(tmp.path(), ConfigLanguage::Python);
    assert_eq!(overridden.statements_per_function, 61);

    let missing = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(missing.path()).unwrap();

    let fallback = Config::load_for_language_with_override(missing.path(), ConfigLanguage::Python);
    let baseline = Config::load_for_language(ConfigLanguage::Python);
    assert_eq!(
        fallback.statements_per_function,
        baseline.statements_per_function
    );
}

#[test]
fn test_load_from_for_language_and_try_load_from() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[python]\nstatements_per_function = 77\n").unwrap();

    let loaded = Config::load_from_for_language(tmp.path(), ConfigLanguage::Python);
    assert_eq!(loaded.statements_per_function, 77);

    let try_loaded = Config::try_load_from(tmp.path(), ConfigLanguage::Python).unwrap();
    assert_eq!(try_loaded.statements_per_function, 77);

    let missing = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(missing.path()).unwrap();
    let err = Config::try_load_from(missing.path(), ConfigLanguage::Rust).unwrap_err();
    assert!(matches!(err, ConfigError::IoError { .. }));
}

#[test]
fn merge_applies_aliases_and_language_filtering() {
    let mut config = Config::python_defaults();
    config.merge_from_toml(
        "\
[thresholds]
classes_per_file = 12
[shared]
types_per_file = 13
[python]
types_per_file = 14
[rust]
types_per_file = 15
",
        Some(ConfigLanguage::Python),
    );
    assert_eq!(config.concrete_types_per_file, 14);

    let mut rust_config = Config::rust_defaults();
    rust_config.merge_from_toml(
        "\
[python]
statements_per_function = 99
[rust]
types_per_file = 16
attributes_per_function = 17
",
        Some(ConfigLanguage::Rust),
    );
    assert_ne!(rust_config.statements_per_function, 99);
    assert_eq!(rust_config.concrete_types_per_file, 16);
    assert_eq!(rust_config.annotations_per_function, 17);
}

#[test]
fn try_merge_reports_parse_unknown_section_and_unknown_key_errors() {
    let mut config = Config::python_defaults();
    let parse = config
        .try_merge_from_toml("[python\nbad", Some(ConfigLanguage::Python))
        .unwrap_err();
    assert!(matches!(parse, ConfigError::ParseError { .. }));

    let section = config
        .try_merge_from_toml("[pythno]\nvalue = 1", Some(ConfigLanguage::Python))
        .unwrap_err();
    assert!(matches!(section, ConfigError::UnknownSection { .. }));

    let key = config
        .try_merge_from_toml("[python]\nunknown = 1", Some(ConfigLanguage::Python))
        .unwrap_err();
    assert!(matches!(key, ConfigError::UnknownKey { .. }));
}

#[test]
fn merge_from_toml_with_path_ignores_invalid_input_without_mutating() {
    let mut config = Config::python_defaults();
    let before = config.statements_per_function;
    config.merge_from_toml_with_path(
        "[python\nbad",
        Some(ConfigLanguage::Python),
        Some(std::path::Path::new(".kissconfig")),
    );
    assert_eq!(config.statements_per_function, before);

    config.merge_from_toml_with_path(
        "[unknown]\nvalue = 1",
        Some(ConfigLanguage::Python),
        Some(std::path::Path::new(".kissconfig")),
    );
    assert_eq!(config.statements_per_function, before);
}

#[test]
fn config_defaults_and_language_debug_are_stable() {
    let default_config = Config::default();
    assert_eq!(
        default_config.statements_per_function,
        Config::python_defaults().statements_per_function
    );
    assert_ne!(
        Config::python_defaults().arguments_keyword_only,
        Config::rust_defaults().arguments_keyword_only
    );
    assert_eq!(format!("{:?}", ConfigLanguage::Rust), "Rust");
}

#[test]
fn merge_without_language_applies_both_language_sections_in_order() {
    let mut config = Config::python_defaults();
    config.merge_from_toml(
        "\
[python]
positional_args = 11
[rust]
arguments = 12
",
        None,
    );

    assert_eq!(config.arguments_positional, 12);
}

#[test]
fn rust_defaults_expose_not_applicable_python_only_fields() {
    let rust = Config::rust_defaults();

    assert_eq!(rust.arguments_keyword_only, crate::defaults::NOT_APPLICABLE);
    assert_eq!(
        rust.return_values_per_function,
        crate::defaults::NOT_APPLICABLE
    );
    assert_eq!(
        rust.statements_per_try_block,
        crate::defaults::NOT_APPLICABLE
    );
}

#[test]
fn language_tables_present_from_toml_and_missing_language() {
    use super::types::{LanguageTablesPresent, missing_language_table_message};

    assert_eq!(
        LanguageTablesPresent::from_toml("[test]\nnum_jobs = 4\n"),
        LanguageTablesPresent::none()
    );
    let both = LanguageTablesPresent::from_toml("[python]\n[rust]\n");
    assert!(both.python && both.rust);
    let py = [std::path::PathBuf::from("a.py")];
    let rs = [std::path::PathBuf::from("a.rs")];
    assert_eq!(both.missing_language(&py, &rs), None);
    assert_eq!(
        LanguageTablesPresent::none().missing_language(&py, &[]),
        Some("python")
    );
    assert_eq!(
        LanguageTablesPresent::none().missing_language(&[], &rs),
        Some("rust")
    );
    assert!(missing_language_table_message("rust").contains("kiss check"));
}
