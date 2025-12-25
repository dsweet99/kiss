//! Configuration management for kiss

use crate::defaults;
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
    pub fan_out: usize,
    pub fan_in: usize,
    pub lcom: usize, // Stored as percentage (0-100)
}

impl Default for Config {
    fn default() -> Self {
        Self::python_defaults()
    }
}

impl Config {
    /// Python-specific defaults
    pub const fn python_defaults() -> Self {
        Self {
            statements_per_function: defaults::python::STATEMENTS_PER_FUNCTION,
            methods_per_class: defaults::python::METHODS_PER_CLASS,
            lines_per_file: defaults::python::LINES_PER_FILE,
            arguments_per_function: 7, // Not used for Python, keep reasonable default
            arguments_positional: defaults::python::POSITIONAL_ARGS,
            arguments_keyword_only: defaults::python::KEYWORD_ONLY_ARGS,
            max_indentation_depth: defaults::python::MAX_INDENTATION,
            classes_per_file: defaults::python::TYPES_PER_FILE,
            nested_function_depth: defaults::python::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::python::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::python::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::python::LOCAL_VARIABLES,
            imports_per_file: defaults::python::IMPORTS_PER_FILE,
            fan_out: defaults::python::FAN_OUT,
            fan_in: defaults::python::FAN_IN,
            lcom: defaults::python::LCOM,
        }
    }

    /// Rust-specific defaults
    pub const fn rust_defaults() -> Self {
        Self {
            statements_per_function: defaults::rust::STATEMENTS_PER_FUNCTION,
            methods_per_class: defaults::rust::METHODS_PER_TYPE,
            lines_per_file: defaults::rust::LINES_PER_FILE,
            arguments_per_function: defaults::rust::ARGUMENTS,
            arguments_positional: 5, // Not used for Rust
            arguments_keyword_only: 6, // Not used for Rust
            max_indentation_depth: defaults::rust::MAX_INDENTATION,
            classes_per_file: defaults::rust::TYPES_PER_FILE,
            nested_function_depth: defaults::rust::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::rust::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::rust::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::rust::LOCAL_VARIABLES,
            imports_per_file: defaults::rust::IMPORTS_PER_FILE,
            fan_out: defaults::rust::FAN_OUT,
            fan_in: defaults::rust::FAN_IN,
            lcom: defaults::rust::LCOM,
        }
    }

    /// Load config from files, with later files overriding earlier ones.
    /// Loads from: ~/.kissconfig, ./.kissconfig
    /// This loads ALL sections (legacy behavior for backwards compatibility).
    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content, None);
            }
        }
        let local_config = Path::new(".kissconfig");
        if let Ok(content) = std::fs::read_to_string(local_config) {
            config.merge_from_toml(&content, None);
        }

        config
    }

    /// Load config for a specific language.
    /// Only applies [thresholds], [shared], and the specified language section.
    pub fn load_for_language(lang: ConfigLanguage) -> Self {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content, Some(lang));
            }
        }
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
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };

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
        if let Some(thresholds) = table.get("thresholds").and_then(|v| v.as_table()) {
            self.apply_thresholds(thresholds);
        }
        if let Some(shared) = table.get("shared").and_then(|v| v.as_table()) {
            self.apply_shared(shared);
        }
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
            "fan_out" => fan_out,
            "fan_in" => fan_in,
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
            "fan_out" => fan_out,
            "fan_in" => fan_in,
            "lcom" => lcom,
            "returns_per_function" => returns_per_function,
            "nested_function_depth" => nested_function_depth
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
            "fan_out" => fan_out,
            "fan_in" => fan_in,
            "lcom" => lcom,
            "lines_per_file" => lines_per_file,
            "types_per_file" => classes_per_file,
            "returns_per_function" => returns_per_function,
            "nested_function_depth" => nested_function_depth
        );
    }
}

fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    table.get(key)
        .and_then(toml::Value::as_integer)
        .filter(|&v| v >= 0)  // Ignore negative values
        .map(|v| v as usize)
}

/// Gate configuration for test coverage requirements
#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize, // percentage (0-100)
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD,
        }
    }
}

impl GateConfig {
    /// Load gate config from standard config files
    pub fn load() -> Self {
        let mut config = Self::default();
        
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content);
            }
        }
        
        let local_config = Path::new(".kissconfig");
        if let Ok(content) = std::fs::read_to_string(local_config) {
            config.merge_from_toml(&content);
        }
        
        config
    }
    
    /// Load gate config from a specific file
    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml(&content);
        }
        config
    }
    
    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else { return };
        
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table())
            && let Some(thresh) = get_usize(gate, "test_coverage_threshold") {
                self.test_coverage_threshold = thresh.min(100); // Cap at 100%
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_and_thresholds() {
        let py = Config::python_defaults();
        let rs = Config::rust_defaults();
        assert_eq!(py.statements_per_function, defaults::python::STATEMENTS_PER_FUNCTION);
        assert_eq!(rs.statements_per_function, defaults::rust::STATEMENTS_PER_FUNCTION);
        assert!(py.lines_per_file > 0);
        assert!(rs.lines_per_file > 0);
        assert_ne!(ConfigLanguage::Python, ConfigLanguage::Rust);
    }

    #[test]
    fn test_merge_overrides_and_edge_cases() {
        let mut c = Config::default();
        let orig = c.statements_per_function;
        c.merge_from_toml("[thresholds]\nstatements_per_function = 100\nmethods_per_class = 30", None);
        assert_eq!(c.statements_per_function, 100);
        assert_eq!(c.methods_per_class, 30);
        let mut c2 = Config::default();
        c2.merge_from_toml("invalid {{{{", None);
        assert_eq!(c2.statements_per_function, orig);
        c2.merge_from_toml("[other]\nx = 1", None);
        assert_eq!(c2.statements_per_function, orig);
        c2.merge_from_toml("[thresholds]\nstatements_per_function = -1", None);
        assert_eq!(c2.statements_per_function, orig);
        c2.merge_from_toml("[thresholds]\nstatements_per_function = \"bad\"", None);
        assert_eq!(c2.statements_per_function, orig);
    }

    #[test]
    fn test_python_section() {
        let mut c = Config::default();
        c.merge_from_toml("[python]\nstatements_per_function = 60\npositional_args = 4\nkeyword_only_args = 8\nmax_indentation = 5", Some(ConfigLanguage::Python));
        assert_eq!(c.statements_per_function, 60);
        assert_eq!(c.arguments_positional, 4);
        assert_eq!(c.max_indentation_depth, 5);
    }

    #[test]
    fn test_rust_section() {
        let mut c = Config::default();
        c.merge_from_toml("[rust]\nstatements_per_function = 70\narguments = 6\nmax_indentation = 5\nmethods_per_type = 25", Some(ConfigLanguage::Rust));
        assert_eq!(c.statements_per_function, 70);
        assert_eq!(c.arguments_per_function, 6);
        assert_eq!(c.methods_per_class, 25);
    }

    #[test]
    fn test_shared_and_language_isolation() {
        let mut c = Config::default();
        c.merge_from_toml("[shared]\nlines_per_file = 600\ntypes_per_file = 4", None);
        assert_eq!(c.lines_per_file, 600);
        let toml = "[python]\nstatements_per_function = 40\n[rust]\nstatements_per_function = 80\n[shared]\nlines_per_file = 700";
        let mut py = Config::default(); py.merge_from_toml(toml, Some(ConfigLanguage::Python));
        let mut rs = Config::default(); rs.merge_from_toml(toml, Some(ConfigLanguage::Rust));
        assert_eq!(py.statements_per_function, 40);
        assert_eq!(rs.statements_per_function, 80);
        assert_eq!(py.lines_per_file, 700);
        assert_eq!(rs.lines_per_file, 700);
    }

    #[test]
    fn test_load_functions() {
        assert!(Config::load().statements_per_function > 0);
        assert!(Config::load_for_language(ConfigLanguage::Python).statements_per_function > 0);
        assert!(Config::load_from(std::path::Path::new("/nonexistent")).statements_per_function > 0);
        assert!(Config::load_from_for_language(std::path::Path::new("/nonexistent"), ConfigLanguage::Rust).statements_per_function > 0);
    }

    #[test]
    fn test_apply_methods() {
        let mut c = Config::default();
        let toml = "[thresholds]\nstatements_per_function = 100\n[shared]\nlines_per_file = 999\n[python]\nstatements_per_function = 55\n[rust]\nstatements_per_function = 66".parse::<toml::Table>().unwrap();
        if let Some(t) = toml.get("thresholds").and_then(|v| v.as_table()) { c.apply_thresholds(t); }
        assert_eq!(c.statements_per_function, 100);
        if let Some(t) = toml.get("shared").and_then(|v| v.as_table()) { c.apply_shared(t); }
        assert_eq!(c.lines_per_file, 999);
        if let Some(t) = toml.get("python").and_then(|v| v.as_table()) { c.apply_python(t); }
        assert_eq!(c.statements_per_function, 55);
        let mut c2 = Config::default();
        if let Some(t) = toml.get("rust").and_then(|v| v.as_table()) { c2.apply_rust(t); }
        assert_eq!(c2.statements_per_function, 66);
        assert_eq!(get_usize(&"x = 42".parse::<toml::Table>().unwrap(), "x"), Some(42));
    }

    // --- Design doc: Configuration Precedence Chain ---
    // "Configurable thresholds are read from config files in this order 
    // (later overrides earlier): 1. ~/.kissconfig 2. ./.kissconfig"

    #[test]
    fn test_local_config_overrides_earlier_values() {
        // Test that later values in merge chain override earlier ones
        let mut config = Config::python_defaults();
        let original = config.statements_per_function;
        
        // First merge: sets to 100
        config.merge_from_toml("[python]\nstatements_per_function = 100", Some(ConfigLanguage::Python));
        assert_eq!(config.statements_per_function, 100);
        
        // Second merge: overrides to 50 (simulating local config override)
        config.merge_from_toml("[python]\nstatements_per_function = 50", Some(ConfigLanguage::Python));
        assert_eq!(config.statements_per_function, 50, "later config should override earlier");
        
        // Verify original was different
        assert_ne!(original, 50);
    }

    #[test]
    fn test_partial_override_preserves_other_values() {
        // Test that overriding one field doesn't affect others
        let mut config = Config::python_defaults();
        let original_lines = config.lines_per_file;
        let original_methods = config.methods_per_class;
        
        // Override only statements_per_function
        config.merge_from_toml("[python]\nstatements_per_function = 999", Some(ConfigLanguage::Python));
        
        assert_eq!(config.statements_per_function, 999, "overridden value should change");
        assert_eq!(config.lines_per_file, original_lines, "unspecified value should be preserved");
        assert_eq!(config.methods_per_class, original_methods, "unspecified value should be preserved");
    }

    #[test]
    fn test_explicit_config_file_takes_precedence() {
        use std::io::Write;
        
        // Create a config file with specific values
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "[python]\nstatements_per_function = 42").unwrap();
        
        let config = Config::load_from_for_language(tmp.path(), ConfigLanguage::Python);
        assert_eq!(config.statements_per_function, 42, "--config file should set value");
    }

    // --- Design doc: Language-Specific Default Differences ---
    // "Shows different thresholds for Python vs Rust"

    #[test]
    fn test_python_and_rust_defaults_differ() {
        let py = Config::python_defaults();
        let rs = Config::rust_defaults();
        
        // These should differ based on defaults.rs
        assert_ne!(py.statements_per_function, rs.statements_per_function, 
            "Python and Rust should have different statements_per_function defaults");
        assert_ne!(py.classes_per_file, rs.classes_per_file,
            "Python (types_per_file) and Rust (types_per_file) should differ");
    }

    #[test]
    fn test_gate_config_threshold_boundary() {
        // Test that threshold is properly capped at 100
        let mut gate = GateConfig::default();
        gate.merge_from_toml("[gate]\ntest_coverage_threshold = 150");
        assert_eq!(gate.test_coverage_threshold, 100, "threshold should be capped at 100%");
        
        gate.merge_from_toml("[gate]\ntest_coverage_threshold = 0");
        assert_eq!(gate.test_coverage_threshold, 0, "0% threshold should be allowed");
    }
}
