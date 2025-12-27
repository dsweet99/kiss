use kiss::{Config, ConfigLanguage, GateConfig};

#[test]
fn default_config_has_reasonable_values() {
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    assert_eq!(py_config.statements_per_function, 35);
    assert_eq!(py_config.methods_per_class, 20);
    assert_eq!(py_config.statements_per_file, 400);
    assert_eq!(rs_config.statements_per_function, 25);
    assert_eq!(rs_config.methods_per_class, 15);
    assert_eq!(rs_config.statements_per_file, 300);
}

#[test]
fn test_config_language_enum() {
    assert_ne!(ConfigLanguage::Python, ConfigLanguage::Rust);
}

#[test]
fn test_config_struct_fields() {
    let c = Config::default();
    assert!(c.statements_per_function > 0 && c.statements_per_file > 0);
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

#[test]
fn test_gate_config_defaults() {
    let gate = GateConfig::default();
    assert!(gate.test_coverage_threshold > 0);
    assert!(gate.min_similarity > 0.0 && gate.min_similarity <= 1.0);
}

#[test]
fn test_gate_config_load() {
    let gate = GateConfig::load();
    assert!(gate.test_coverage_threshold > 0);
}

#[test]
fn test_load_from_content() {
    let content = "[python]\nstatements_per_function = 99";
    let c = Config::load_from_content(content, ConfigLanguage::Python);
    assert_eq!(c.statements_per_function, 99);
}

#[test]
fn test_is_similar() {
    assert!(kiss::is_similar("pytohn", "python"));
    assert!(kiss::is_similar("rus", "rust"));
    assert!(!kiss::is_similar("xyz", "python"));
}
