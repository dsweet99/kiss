
use kiss::{Config, ConfigLanguage};

#[test]
fn default_config_has_reasonable_values() {
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    
    assert_eq!(py_config.statements_per_function, 35);
    assert_eq!(py_config.methods_per_class, 20);
    assert_eq!(py_config.lines_per_file, 300);
    
    assert_eq!(rs_config.statements_per_function, 25);
    assert_eq!(rs_config.methods_per_class, 15);
    assert_eq!(rs_config.lines_per_file, 300);
}

#[test]
fn test_config_language_enum() {
    assert_ne!(ConfigLanguage::Python, ConfigLanguage::Rust);
}

#[test]
fn test_config_struct_fields() {
    let c = Config::default();
    assert!(c.statements_per_function > 0);
    assert!(c.lines_per_file > 0);
}

#[test]
fn test_load_returns_config() {
    let c = Config::load();
    assert!(c.statements_per_function > 0);
}

#[test]
fn test_load_for_language() {
    let c = Config::load_for_language(ConfigLanguage::Python);
    assert!(c.statements_per_function > 0);
}

#[test]
fn test_load_from_nonexistent() {
    let c = Config::load_from(std::path::Path::new("/nonexistent/path"));
    assert!(c.statements_per_function > 0);
}

#[test]
fn test_load_from_for_language() {
    let c = Config::load_from_for_language(std::path::Path::new("/nonexistent"), ConfigLanguage::Rust);
    assert!(c.statements_per_function > 0);
}
