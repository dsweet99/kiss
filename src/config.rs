//! Configuration management for kiss

use std::path::Path;

/// Default thresholds for count metrics
pub mod thresholds {
    pub const STATEMENTS_PER_FUNCTION: usize = 50;
    pub const METHODS_PER_CLASS: usize = 20;
    pub const LINES_PER_FILE: usize = 500;
    pub const ARGUMENTS_PER_FUNCTION: usize = 5;
    pub const MAX_INDENTATION_DEPTH: usize = 4;
    pub const CLASSES_PER_FILE: usize = 3;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const BRANCHES_PER_FUNCTION: usize = 10;
    pub const LOCAL_VARIABLES_PER_FUNCTION: usize = 10;
    pub const IMPORTS_PER_FILE: usize = 15;
    pub const CYCLOMATIC_COMPLEXITY: usize = 10;
    pub const FAN_OUT: usize = 10;
}

/// Configuration for kiss thresholds
#[derive(Debug, Clone)]
pub struct Config {
    pub statements_per_function: usize,
    pub methods_per_class: usize,
    pub lines_per_file: usize,
    pub arguments_per_function: usize,
    pub max_indentation_depth: usize,
    pub classes_per_file: usize,
    pub nested_function_depth: usize,
    pub returns_per_function: usize,
    pub branches_per_function: usize,
    pub local_variables_per_function: usize,
    pub imports_per_file: usize,
    pub cyclomatic_complexity: usize,
    pub fan_out: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            statements_per_function: thresholds::STATEMENTS_PER_FUNCTION,
            methods_per_class: thresholds::METHODS_PER_CLASS,
            lines_per_file: thresholds::LINES_PER_FILE,
            arguments_per_function: thresholds::ARGUMENTS_PER_FUNCTION,
            max_indentation_depth: thresholds::MAX_INDENTATION_DEPTH,
            classes_per_file: thresholds::CLASSES_PER_FILE,
            nested_function_depth: thresholds::NESTED_FUNCTION_DEPTH,
            returns_per_function: thresholds::RETURNS_PER_FUNCTION,
            branches_per_function: thresholds::BRANCHES_PER_FUNCTION,
            local_variables_per_function: thresholds::LOCAL_VARIABLES_PER_FUNCTION,
            imports_per_file: thresholds::IMPORTS_PER_FILE,
            cyclomatic_complexity: thresholds::CYCLOMATIC_COMPLEXITY,
            fan_out: thresholds::FAN_OUT,
        }
    }
}

impl Config {
    /// Load config from files, with later files overriding earlier ones.
    /// Loads from: ~/.kissconfig, ./.kissconfig
    pub fn load() -> Self {
        let mut config = Self::default();

        // Try home directory config
        if let Some(home) = std::env::var_os("HOME") {
            let home_config = Path::new(&home).join(".kissconfig");
            if let Ok(content) = std::fs::read_to_string(&home_config) {
                config.merge_from_toml(&content);
            }
        }

        // Try local config (overrides home config)
        let local_config = Path::new(".kissconfig");
        if let Ok(content) = std::fs::read_to_string(local_config) {
            config.merge_from_toml(&content);
        }

        config
    }

    /// Merge values from a TOML string into this config
    fn merge_from_toml(&mut self, content: &str) {
        let Ok(table) = content.parse::<toml::Table>() else {
            return;
        };
        let Some(thresholds) = table.get("thresholds").and_then(|v| v.as_table()) else {
            return;
        };

        fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
            table.get(key)
                .and_then(|v| v.as_integer())
                .filter(|&v| v >= 0)  // Ignore negative values
                .map(|v| v as usize)
        }

        if let Some(v) = get_usize(thresholds, "statements_per_function") {
            self.statements_per_function = v;
        }
        if let Some(v) = get_usize(thresholds, "methods_per_class") {
            self.methods_per_class = v;
        }
        if let Some(v) = get_usize(thresholds, "lines_per_file") {
            self.lines_per_file = v;
        }
        if let Some(v) = get_usize(thresholds, "arguments_per_function") {
            self.arguments_per_function = v;
        }
        if let Some(v) = get_usize(thresholds, "max_indentation_depth") {
            self.max_indentation_depth = v;
        }
        if let Some(v) = get_usize(thresholds, "classes_per_file") {
            self.classes_per_file = v;
        }
        if let Some(v) = get_usize(thresholds, "nested_function_depth") {
            self.nested_function_depth = v;
        }
        if let Some(v) = get_usize(thresholds, "returns_per_function") {
            self.returns_per_function = v;
        }
        if let Some(v) = get_usize(thresholds, "branches_per_function") {
            self.branches_per_function = v;
        }
        if let Some(v) = get_usize(thresholds, "local_variables_per_function") {
            self.local_variables_per_function = v;
        }
        if let Some(v) = get_usize(thresholds, "imports_per_file") {
            self.imports_per_file = v;
        }
        if let Some(v) = get_usize(thresholds, "cyclomatic_complexity") {
            self.cyclomatic_complexity = v;
        }
        if let Some(v) = get_usize(thresholds, "fan_out") {
            self.fan_out = v;
        }
    }
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
        config.merge_from_toml(toml);
        assert_eq!(config.statements_per_function, 100);
        assert_eq!(config.methods_per_class, 30);
        // Other values should remain at default
        assert_eq!(config.lines_per_file, thresholds::LINES_PER_FILE);
    }

    #[test]
    fn merge_from_toml_ignores_malformed_toml() {
        let mut config = Config::default();
        let original_statements = config.statements_per_function;
        config.merge_from_toml("this is not valid toml {{{{");
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
        config.merge_from_toml(toml);
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
        config.merge_from_toml(toml);
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
        config.merge_from_toml(toml);
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
        config.merge_from_toml(toml);
        // Only the specified value should change
        assert_eq!(config.cyclomatic_complexity, 15);
        assert_eq!(config.statements_per_function, thresholds::STATEMENTS_PER_FUNCTION);
    }
}

