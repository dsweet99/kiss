//! Configuration management for kiss

use std::path::Path;

/// Macro to reduce boilerplate in apply_* methods
macro_rules! apply_config {
    ($self:ident, $table:ident, $($key:literal => $field:ident),+ $(,)?) => {
        $( if let Some(v) = get_usize($table, $key) { $self.$field = v; } )+
    };
}

/// Language for config loading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLanguage {
    Python,
    Rust,
}

/// Default thresholds for count metrics
pub mod thresholds {
    pub const STATEMENTS_PER_FUNCTION: usize = 50;
    pub const METHODS_PER_CLASS: usize = 20;
    pub const LINES_PER_FILE: usize = 500;
    pub const ARGUMENTS_PER_FUNCTION: usize = 7;
    pub const ARGUMENTS_POSITIONAL: usize = 3;
    pub const ARGUMENTS_KEYWORD_ONLY: usize = 6;
    pub const MAX_INDENTATION_DEPTH: usize = 4;
    pub const CLASSES_PER_FILE: usize = 3;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const BRANCHES_PER_FUNCTION: usize = 10;
    pub const LOCAL_VARIABLES_PER_FUNCTION: usize = 10;
    pub const IMPORTS_PER_FILE: usize = 15;
    pub const CYCLOMATIC_COMPLEXITY: usize = 10;
    pub const FAN_OUT: usize = 10;
    pub const FAN_IN: usize = 20;
    pub const TRANSITIVE_DEPS: usize = 30;
    pub const LCOM: usize = 50; // Stored as percentage (0-100), threshold > 50%
}

/// Configuration for kiss thresholds
#[derive(Debug, Clone)]
pub struct Config {
    pub statements_per_function: usize,
    pub methods_per_class: usize,
    pub lines_per_file: usize,
    pub arguments_per_function: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation_depth: usize,
    pub classes_per_file: usize,
    pub nested_function_depth: usize,
    pub returns_per_function: usize,
    pub branches_per_function: usize,
    pub local_variables_per_function: usize,
    pub imports_per_file: usize,
    pub cyclomatic_complexity: usize,
    pub fan_out: usize,
    pub fan_in: usize,
    pub transitive_deps: usize,
    pub lcom: usize, // Stored as percentage (0-100)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            statements_per_function: thresholds::STATEMENTS_PER_FUNCTION,
            methods_per_class: thresholds::METHODS_PER_CLASS,
            lines_per_file: thresholds::LINES_PER_FILE,
            arguments_per_function: thresholds::ARGUMENTS_PER_FUNCTION,
            arguments_positional: thresholds::ARGUMENTS_POSITIONAL,
            arguments_keyword_only: thresholds::ARGUMENTS_KEYWORD_ONLY,
            max_indentation_depth: thresholds::MAX_INDENTATION_DEPTH,
            classes_per_file: thresholds::CLASSES_PER_FILE,
            nested_function_depth: thresholds::NESTED_FUNCTION_DEPTH,
            returns_per_function: thresholds::RETURNS_PER_FUNCTION,
            branches_per_function: thresholds::BRANCHES_PER_FUNCTION,
            local_variables_per_function: thresholds::LOCAL_VARIABLES_PER_FUNCTION,
            imports_per_file: thresholds::IMPORTS_PER_FILE,
            cyclomatic_complexity: thresholds::CYCLOMATIC_COMPLEXITY,
            fan_out: thresholds::FAN_OUT,
            fan_in: thresholds::FAN_IN,
            transitive_deps: thresholds::TRANSITIVE_DEPS,
            lcom: thresholds::LCOM,
        }
    }
}

impl Config {
    /// Load config from files, with later files overriding earlier ones.
    /// Loads from: ~/.kissconfig, ./.kissconfig
    /// This loads ALL sections (legacy behavior for backwards compatibility).
    pub fn load() -> Self {
        let mut config = Self::default();

        // Try home directory config
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content, None);
            }
        }

        // Try local config (overrides home config)
        let local_config = Path::new(".kissconfig");
        if let Ok(content) = std::fs::read_to_string(local_config) {
            config.merge_from_toml(&content, None);
        }

        config
    }

    /// Load config for a specific language.
    /// Only applies [thresholds], [shared], and the specified language section.
    pub fn load_for_language(lang: ConfigLanguage) -> Self {
        let mut config = Self::default();

        // Try home directory config
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content, Some(lang));
            }
        }

        // Try local config (overrides home config)
        let local_config = Path::new(".kissconfig");
        if let Ok(content) = std::fs::read_to_string(local_config) {
            config.merge_from_toml(&content, Some(lang));
        }

        config
    }

    /// Load config from a specific file path
    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();

        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml(&content, None);
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }

        config
    }

    /// Load config from a specific file path for a specific language
    pub fn load_from_for_language(path: &Path, lang: ConfigLanguage) -> Self {
        let mut config = Self::default();

        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml(&content, Some(lang));
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }

        config
    }

    /// Merge values from a TOML string into this config.
    /// If `lang` is Some, only applies [thresholds], [shared], and the specified language section.
    /// If `lang` is None, applies all sections (legacy behavior).
    fn merge_from_toml(&mut self, content: &str, lang: Option<ConfigLanguage>) {
        let Ok(table) = content.parse::<toml::Table>() else {
            return;
        };

        // Try legacy [thresholds] section first
        if let Some(thresholds) = table.get("thresholds").and_then(|v| v.as_table()) {
            self.apply_thresholds(thresholds);
        }

        // Apply [shared] section (overrides thresholds)
        if let Some(shared) = table.get("shared").and_then(|v| v.as_table()) {
            self.apply_shared(shared);
        }

        // Apply language-specific section based on lang parameter
        match lang {
            Some(ConfigLanguage::Python) => {
                if let Some(python) = table.get("python").and_then(|v| v.as_table()) {
                    self.apply_python(python);
                }
            }
            Some(ConfigLanguage::Rust) => {
                if let Some(rust) = table.get("rust").and_then(|v| v.as_table()) {
                    self.apply_rust(rust);
                }
            }
            None => {
                // Legacy: apply both (last one wins for overlapping fields)
                if let Some(python) = table.get("python").and_then(|v| v.as_table()) {
                    self.apply_python(python);
                }
                if let Some(rust) = table.get("rust").and_then(|v| v.as_table()) {
                    self.apply_rust(rust);
                }
            }
        }
    }

    fn apply_thresholds(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "statements_per_function" => statements_per_function,
            "methods_per_class" => methods_per_class,
            "lines_per_file" => lines_per_file,
            "arguments_per_function" => arguments_per_function,
            "arguments_positional" => arguments_positional,
            "arguments_keyword_only" => arguments_keyword_only,
            "max_indentation_depth" => max_indentation_depth,
            "classes_per_file" => classes_per_file,
            "nested_function_depth" => nested_function_depth,
            "returns_per_function" => returns_per_function,
            "branches_per_function" => branches_per_function,
            "local_variables_per_function" => local_variables_per_function,
            "imports_per_file" => imports_per_file,
            "cyclomatic_complexity" => cyclomatic_complexity,
            "fan_out" => fan_out,
            "fan_in" => fan_in,
            "transitive_deps" => transitive_deps,
            "lcom" => lcom
        );
    }

    fn apply_shared(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "lines_per_file" => lines_per_file,
            "types_per_file" => classes_per_file,
            "imports_per_file" => imports_per_file
        );
    }

    fn apply_python(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "statements_per_function" => statements_per_function,
            "positional_args" => arguments_positional,
            "keyword_only_args" => arguments_keyword_only,
            "max_indentation" => max_indentation_depth,
            "branches_per_function" => branches_per_function,
            "local_variables" => local_variables_per_function,
            "methods_per_class" => methods_per_class,
            "cyclomatic_complexity" => cyclomatic_complexity,
            "fan_out" => fan_out,
            "fan_in" => fan_in,
            "transitive_deps" => transitive_deps,
            "lcom" => lcom
        );
    }

    fn apply_rust(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "statements_per_function" => statements_per_function,
            "arguments" => arguments_per_function,
            "max_indentation" => max_indentation_depth,
            "branches_per_function" => branches_per_function,
            "local_variables" => local_variables_per_function,
            "methods_per_type" => methods_per_class,
            "cyclomatic_complexity" => cyclomatic_complexity,
            "fan_out" => fan_out,
            "fan_in" => fan_in,
            "transitive_deps" => transitive_deps,
            "lcom" => lcom
        );
    }
}

fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    table.get(key)
        .and_then(|v| v.as_integer())
        .filter(|&v| v >= 0)  // Ignore negative values
        .map(|v| v as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_threshold_constants() {
        let config = Config::default();
        assert_eq!(config.statements_per_function, thresholds::STATEMENTS_PER_FUNCTION);
        assert_eq!(config.methods_per_class, thresholds::METHODS_PER_CLASS);
        assert_eq!(config.lines_per_file, thresholds::LINES_PER_FILE);
    }

    #[test]
    fn merge_from_toml_overrides_values() {
        let mut config = Config::default();
        let toml = r#"
[thresholds]
statements_per_function = 100
methods_per_class = 30
"#;
        config.merge_from_toml(toml, None);
        assert_eq!(config.statements_per_function, 100);
        assert_eq!(config.methods_per_class, 30);
        // Other values should remain at default
        assert_eq!(config.lines_per_file, thresholds::LINES_PER_FILE);
    }

    #[test]
    fn merge_from_toml_ignores_malformed_toml() {
        let mut config = Config::default();
        let original_statements = config.statements_per_function;
        config.merge_from_toml("this is not valid toml {{{{", None);
        // Should remain unchanged
        assert_eq!(config.statements_per_function, original_statements);
    }

    #[test]
    fn merge_from_toml_ignores_missing_thresholds_section() {
        let mut config = Config::default();
        let original_statements = config.statements_per_function;
        let toml = r#"
[other_section]
some_key = 123
"#;
        config.merge_from_toml(toml, None);
        // Should remain unchanged
        assert_eq!(config.statements_per_function, original_statements);
    }

    #[test]
    fn merge_from_toml_ignores_negative_values() {
        let mut config = Config::default();
        let original_statements = config.statements_per_function;
        let toml = r#"
[thresholds]
statements_per_function = -1
"#;
        config.merge_from_toml(toml, None);
        // Negative values should be ignored, keeping the original
        assert_eq!(config.statements_per_function, original_statements);
    }

    #[test]
    fn merge_from_toml_ignores_wrong_types() {
        let mut config = Config::default();
        let original_statements = config.statements_per_function;
        let toml = r#"
[thresholds]
statements_per_function = "not a number"
"#;
        config.merge_from_toml(toml, None);
        // Wrong types should be ignored
        assert_eq!(config.statements_per_function, original_statements);
    }

    #[test]
    fn merge_from_toml_handles_partial_config() {
        let mut config = Config::default();
        let toml = r#"
[thresholds]
cyclomatic_complexity = 15
"#;
        config.merge_from_toml(toml, None);
        // Only the specified value should change
        assert_eq!(config.cyclomatic_complexity, 15);
        assert_eq!(config.statements_per_function, thresholds::STATEMENTS_PER_FUNCTION);
    }

    #[test]
    fn merge_from_toml_supports_python_section() {
        let mut config = Config::default();
        let toml = r#"
[python]
statements_per_function = 60
positional_args = 4
keyword_only_args = 8
max_indentation = 5
"#;
        config.merge_from_toml(toml, Some(ConfigLanguage::Python));
        assert_eq!(config.statements_per_function, 60);
        assert_eq!(config.arguments_positional, 4);
        assert_eq!(config.arguments_keyword_only, 8);
        assert_eq!(config.max_indentation_depth, 5);
    }

    #[test]
    fn merge_from_toml_supports_rust_section() {
        let mut config = Config::default();
        let toml = r#"
[rust]
statements_per_function = 70
arguments = 6
max_indentation = 5
methods_per_type = 25
"#;
        config.merge_from_toml(toml, Some(ConfigLanguage::Rust));
        assert_eq!(config.statements_per_function, 70);
        assert_eq!(config.arguments_per_function, 6);
        assert_eq!(config.max_indentation_depth, 5);
        assert_eq!(config.methods_per_class, 25);
    }

    #[test]
    fn merge_from_toml_supports_shared_section() {
        let mut config = Config::default();
        let toml = r#"
[shared]
lines_per_file = 600
types_per_file = 4
imports_per_file = 20
"#;
        config.merge_from_toml(toml, None);
        assert_eq!(config.lines_per_file, 600);
        assert_eq!(config.classes_per_file, 4);
        assert_eq!(config.imports_per_file, 20);
    }

    #[test]
    fn merge_from_toml_language_overrides_shared() {
        let mut config = Config::default();
        // Python section should override shared
        let toml = r#"
[shared]
lines_per_file = 600

[python]
statements_per_function = 40
"#;
        config.merge_from_toml(toml, Some(ConfigLanguage::Python));
        assert_eq!(config.lines_per_file, 600);
        assert_eq!(config.statements_per_function, 40);
    }

    #[test]
    fn language_specific_loading_isolates_sections() {
        // This is the key test: when loading for Python, Rust section should NOT be applied
        let toml = r#"
[python]
statements_per_function = 40

[rust]
statements_per_function = 80
"#;
        // Load for Python - should get Python's value
        let mut py_config = Config::default();
        py_config.merge_from_toml(toml, Some(ConfigLanguage::Python));
        assert_eq!(py_config.statements_per_function, 40);

        // Load for Rust - should get Rust's value
        let mut rs_config = Config::default();
        rs_config.merge_from_toml(toml, Some(ConfigLanguage::Rust));
        assert_eq!(rs_config.statements_per_function, 80);
    }

    #[test]
    fn shared_section_applies_to_both_languages() {
        let toml = r#"
[shared]
lines_per_file = 700

[python]
statements_per_function = 40

[rust]
statements_per_function = 80
"#;
        // Both should get shared value
        let mut py_config = Config::default();
        py_config.merge_from_toml(toml, Some(ConfigLanguage::Python));
        assert_eq!(py_config.lines_per_file, 700);
        assert_eq!(py_config.statements_per_function, 40);

        let mut rs_config = Config::default();
        rs_config.merge_from_toml(toml, Some(ConfigLanguage::Rust));
        assert_eq!(rs_config.lines_per_file, 700);
        assert_eq!(rs_config.statements_per_function, 80);
    }

    #[test]
    fn test_config_language_enum() {
        assert_ne!(ConfigLanguage::Python, ConfigLanguage::Rust);
        let _p = ConfigLanguage::Python;
        let _r = ConfigLanguage::Rust;
    }

    #[test]
    fn test_config_struct_fields() {
        let c = Config::default();
        assert!(c.statements_per_function > 0);
        assert!(c.lines_per_file > 0);
    }

    #[test]
    fn test_load_returns_config() {
        // Just verify it doesn't panic
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
        // Should return default config
        assert!(c.statements_per_function > 0);
    }

    #[test]
    fn test_load_from_for_language() {
        let c = Config::load_from_for_language(std::path::Path::new("/nonexistent"), ConfigLanguage::Rust);
        assert!(c.statements_per_function > 0);
    }

    #[test]
    fn test_apply_thresholds() {
        let mut c = Config::default();
        let toml = "[thresholds]\nstatements_per_function = 100".parse::<toml::Table>().unwrap();
        if let Some(t) = toml.get("thresholds").and_then(|v| v.as_table()) {
            c.apply_thresholds(t);
        }
        assert_eq!(c.statements_per_function, 100);
    }

    #[test]
    fn test_apply_shared() {
        let mut c = Config::default();
        let toml = "[shared]\nlines_per_file = 999".parse::<toml::Table>().unwrap();
        if let Some(t) = toml.get("shared").and_then(|v| v.as_table()) {
            c.apply_shared(t);
        }
        assert_eq!(c.lines_per_file, 999);
    }

    #[test]
    fn test_apply_python() {
        let mut c = Config::default();
        let toml = "[python]\nstatements_per_function = 55".parse::<toml::Table>().unwrap();
        if let Some(t) = toml.get("python").and_then(|v| v.as_table()) {
            c.apply_python(t);
        }
        assert_eq!(c.statements_per_function, 55);
    }

    #[test]
    fn test_apply_rust() {
        let mut c = Config::default();
        let toml = "[rust]\nstatements_per_function = 66".parse::<toml::Table>().unwrap();
        if let Some(t) = toml.get("rust").and_then(|v| v.as_table()) {
            c.apply_rust(t);
        }
        assert_eq!(c.statements_per_function, 66);
    }

    #[test]
    fn test_get_usize() {
        let toml = "x = 42".parse::<toml::Table>().unwrap();
        assert_eq!(get_usize(&toml, "x"), Some(42));
        assert_eq!(get_usize(&toml, "y"), None);
    }
}

