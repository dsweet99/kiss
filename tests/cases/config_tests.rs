use kiss::{Config, ConfigLanguage, GateConfig, default_config_toml};

#[test]
fn default_config_has_reasonable_values() {
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    assert_eq!(py_config.statements_per_function, 35);
    assert_eq!(py_config.methods_per_class, 10);
    assert_eq!(py_config.statements_per_file, 200);
    assert_eq!(py_config.lines_per_file, 300);
    assert_eq!(py_config.arguments_positional, 3);
    assert_eq!(py_config.arguments_keyword_only, 3);
    assert_eq!(rs_config.statements_per_function, 35);
    assert_eq!(rs_config.methods_per_class, 10);
    assert_eq!(rs_config.statements_per_file, 250);
    assert_eq!(rs_config.lines_per_file, 900);
    assert_eq!(rs_config.arguments_per_function, 8);
}

#[test]
fn default_config_toml_matches_requested_defaults() {
    let content = default_config_toml();
    let py_config = Config::load_from_content(&content, ConfigLanguage::Python);
    let rs_config = Config::load_from_content(&content, ConfigLanguage::Rust);
    let gate_config = GateConfig::try_load_from_content(&content).unwrap();

    assert_eq!(gate_config.test_coverage_threshold, 90);
    assert!((gate_config.min_similarity - 0.7).abs() < f64::EPSILON);
    assert!(gate_config.duplication_enabled);
    assert!(gate_config.orphan_module_enabled);

    assert_eq!(py_config.statements_per_function, 35);
    assert_eq!(py_config.arguments_positional, 3);
    assert_eq!(py_config.arguments_keyword_only, 3);
    assert_eq!(py_config.max_indentation_depth, 4);
    assert_eq!(py_config.nested_function_depth, 2);
    assert_eq!(py_config.returns_per_function, 5);
    assert_eq!(py_config.return_values_per_function, 3);
    assert_eq!(py_config.branches_per_function, 9);
    assert_eq!(py_config.local_variables_per_function, 15);
    assert_eq!(py_config.statements_per_try_block, 3);
    assert_eq!(py_config.boolean_parameters, 1);
    assert_eq!(py_config.annotations_per_function, 5);
    assert_eq!(py_config.calls_per_function, 20);
    assert_eq!(py_config.statements_per_file, 200);
    assert_eq!(py_config.lines_per_file, 300);
    assert_eq!(py_config.functions_per_file, 10);
    assert_eq!(py_config.interface_types_per_file, 1);
    assert_eq!(py_config.concrete_types_per_file, 1);
    assert_eq!(py_config.imported_names_per_file, 30);
    assert_eq!(py_config.cycle_size, 0);
    assert_eq!(py_config.indirect_dependencies, 10);
    assert_eq!(py_config.dependency_depth, 3);

    assert_eq!(rs_config.statements_per_function, 35);
    assert_eq!(rs_config.arguments_per_function, 8);
    assert_eq!(rs_config.max_indentation_depth, 5);
    assert_eq!(rs_config.nested_function_depth, 2);
    assert_eq!(rs_config.returns_per_function, 5);
    assert_eq!(rs_config.branches_per_function, 9);
    assert_eq!(rs_config.local_variables_per_function, 20);
    assert_eq!(rs_config.boolean_parameters, 1);
    assert_eq!(rs_config.annotations_per_function, 1);
    assert_eq!(rs_config.calls_per_function, 45);
    assert_eq!(rs_config.methods_per_class, 10);
    assert_eq!(rs_config.statements_per_file, 250);
    assert_eq!(rs_config.lines_per_file, 900);
    assert_eq!(rs_config.functions_per_file, 40);
    assert_eq!(rs_config.interface_types_per_file, 0);
    assert_eq!(rs_config.concrete_types_per_file, 8);
    assert_eq!(rs_config.imported_names_per_file, 50);
    assert_eq!(rs_config.cycle_size, 0);
    assert_eq!(rs_config.indirect_dependencies, 10);
    assert_eq!(rs_config.dependency_depth, 3);
}

#[test]
fn test_config_language_enum() {
    assert_ne!(ConfigLanguage::Python, ConfigLanguage::Rust);
}

#[test]
fn test_config_struct_fields() {
    let c = Config::default();
    assert!(
        c.statements_per_function > 0 && c.statements_per_file > 0 && c.lines_per_file > 0
    );
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
    let c =
        Config::load_from_for_language(std::path::Path::new("/nonexistent"), ConfigLanguage::Rust);
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
