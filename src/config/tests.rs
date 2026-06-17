use super::ConfigError;
use super::merge::{apply_python, apply_rust, apply_shared, apply_thresholds};
use super::types::{Config, ConfigLanguage};
use super::validation::{
    check_unknown_keys, check_unknown_sections, get_usize, validate_config_keys,
    validate_python_keys, validate_rust_keys, validate_shared_keys, validate_thresholds_keys,
};
use std::sync::{Mutex, MutexGuard};

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn cwd_lock() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap()
}

#[test]
fn load_from_content_applies_language_defaults_and_toml() {
    let py = Config::load_from_content(
        "[python]\nstatements_per_function = 77",
        ConfigLanguage::Python,
    );
    let rs =
        Config::load_from_content("[rust]\nstatements_per_function = 88", ConfigLanguage::Rust);
    assert_eq!(py.statements_per_function, 77);
    assert_eq!(rs.statements_per_function, 88);
}

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
fn load_apis_apply_present_files_and_preserve_defaults_on_missing_files() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        "[thresholds]\nstatements_per_function = 66\n[rust]\narguments = 7\n",
    )
    .unwrap();

    let all = Config::load_from(tmp.path());
    assert_eq!(all.statements_per_function, 66);
    let rust = Config::load_from_for_language(tmp.path(), ConfigLanguage::Rust);
    assert_eq!(rust.arguments_positional, 7);

    let missing = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(missing.path()).unwrap();
    let default_all = Config::load_from(missing.path());
    assert_eq!(
        default_all.statements_per_function,
        Config::default().statements_per_function
    );
    let default_rust = Config::load_from_for_language(missing.path(), ConfigLanguage::Rust);
    assert_eq!(
        default_rust.arguments_positional,
        Config::rust_defaults().arguments_positional
    );
}

#[test]
fn load_chain_reads_dot_kissconfig_for_default_and_language_loads() {
    let _cwd_guard = cwd_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join(".kissconfig"),
        "[thresholds]\nstatements_per_function = 55\n[rust]\narguments = 6\n",
    )
    .unwrap();

    let all = Config::load();
    let rust = Config::load_for_language(ConfigLanguage::Rust);

    std::env::set_current_dir(original).unwrap();
    assert_eq!(all.statements_per_function, 55);
    assert_eq!(rust.arguments_positional, 6);
}

#[test]
fn load_for_language_with_override_merges_existing_file_and_ignores_missing_override() {
    let _cwd_guard = cwd_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::write(tmp.path().join(".kissconfig"), "[rust]\narguments = 4\n").unwrap();
    let override_path = tmp.path().join("override.toml");
    std::fs::write(&override_path, "[rust]\narguments = 9\n").unwrap();

    let overridden = Config::load_for_language_with_override(&override_path, ConfigLanguage::Rust);
    let missing = Config::load_for_language_with_override(
        &tmp.path().join("missing.toml"),
        ConfigLanguage::Rust,
    );

    std::env::set_current_dir(original).unwrap();
    assert_eq!(overridden.arguments_positional, 9);
    assert_eq!(missing.arguments_positional, 4);
}

#[test]
fn apply_config_sections_ignore_unknown_keys_without_mutating_existing_values() {
    let mut table = toml::Table::new();
    table.insert("unknown_key".into(), toml::Value::Integer(1));

    let mut thresholds = Config::default();
    let before = thresholds.statements_per_function;
    apply_thresholds(&mut thresholds, &table);
    assert_eq!(thresholds.statements_per_function, before);

    let mut shared = Config::default();
    let before = shared.lines_per_file;
    apply_shared(&mut shared, &table);
    assert_eq!(shared.lines_per_file, before);

    let mut python = Config::python_defaults();
    let before = python.arguments_positional;
    apply_python(&mut python, &table);
    assert_eq!(python.arguments_positional, before);

    let mut rust = Config::rust_defaults();
    let before = rust.arguments_positional;
    apply_rust(&mut rust, &table);
    assert_eq!(rust.arguments_positional, before);
}

#[test]
fn merge_from_toml_ignores_parse_errors_and_unknown_sections() {
    let mut parse_error = Config::default();
    parse_error.merge_from_toml_with_path(
        "not = [valid",
        None,
        Some(std::path::Path::new("bad.toml")),
    );
    assert_eq!(
        parse_error.statements_per_function,
        Config::default().statements_per_function
    );

    let mut unknown_section = Config::default();
    unknown_section.merge_from_toml("[extra]\nvalue = 1\n", None);
    assert_eq!(
        unknown_section.statements_per_function,
        Config::default().statements_per_function
    );
}
