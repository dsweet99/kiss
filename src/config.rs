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
    pub const TEST_COVERAGE_THRESHOLD: usize = 90; // percentage (0-100)
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
        let mut config = Self::default();
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
            "lcom" => lcom,
            "lines_per_file" => lines_per_file,
            "types_per_file" => classes_per_file
        );
    }
}

fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    table.get(key)
        .and_then(|v| v.as_integer())
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
            test_coverage_threshold: thresholds::TEST_COVERAGE_THRESHOLD,
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
        let c = Config::default();
        assert_eq!(c.statements_per_function, thresholds::STATEMENTS_PER_FUNCTION);
        assert_eq!(c.methods_per_class, thresholds::METHODS_PER_CLASS);
        assert!(c.lines_per_file > 0);
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
        c2.merge_from_toml("[thresholds]\ncyclomatic_complexity = 15", None);
        assert_eq!(c2.cyclomatic_complexity, 15);
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
}
