//! Tests for configuration management

use kiss::{Config, ConfigLanguage, thresholds};

#[test]
fn default_config_uses_threshold_constants() {
    let config = Config::default();
    assert_eq!(config.statements_per_function, thresholds::STATEMENTS_PER_FUNCTION);
    assert_eq!(config.methods_per_class, thresholds::METHODS_PER_CLASS);
    assert_eq!(config.lines_per_file, thresholds::LINES_PER_FILE);
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

