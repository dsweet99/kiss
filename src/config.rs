use crate::defaults;
use std::path::Path;

/// Error type for configuration validation
#[derive(Debug, Clone)]
pub enum ConfigError {
    /// Unknown key in a config section
    UnknownKey { key: String, section: String },
    /// Unknown section in the config file
    UnknownSection { section: String, hint: Option<String> },
    /// Invalid value for a config key
    InvalidValue { key: String, message: String },
    /// Failed to parse TOML content
    ParseError { message: String },
    /// Failed to read config file
    IoError { path: String, message: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKey { key, section } => {
                write!(f, "Unknown config key '{key}' in [{section}]")
            }
            Self::UnknownSection { section, hint } => {
                write!(f, "Unknown config section '[{section}]'")?;
                if let Some(h) = hint {
                    write!(f, " - did you mean '[{h}]'?")?;
                }
                Ok(())
            }
            Self::InvalidValue { key, message } => {
                write!(f, "Invalid value for '{key}': {message}")
            }
            Self::ParseError { message } => {
                write!(f, "Failed to parse config: {message}")
            }
            Self::IoError { path, message } => {
                write!(f, "Failed to read config '{path}': {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

macro_rules! apply_config {
    ($self:ident, $table:ident, $($key:literal => $field:ident),+ $(,)?) => {
        $( if let Some(v) = get_usize($table, $key) { $self.$field = v; } )+
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLanguage {
    Python,
    Rust,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub statements_per_function: usize,
    pub methods_per_class: usize,
    pub statements_per_file: usize,
    pub functions_per_file: usize,
    pub arguments_per_function: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation_depth: usize,
    pub interface_types_per_file: usize,
    pub concrete_types_per_file: usize,
    pub nested_function_depth: usize,
    pub returns_per_function: usize,
    pub return_values_per_function: usize,
    pub branches_per_function: usize,
    pub local_variables_per_function: usize,
    pub imported_names_per_file: usize,
    pub statements_per_try_block: usize,
    pub boolean_parameters: usize,
    pub annotations_per_function: usize,
    pub calls_per_function: usize,
    pub cycle_size: usize,
    pub transitive_dependencies: usize,
    pub dependency_depth: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self::python_defaults()
    }
}

impl Config {
    pub const fn python_defaults() -> Self {
        use defaults::python as py;
        Self {
            statements_per_function: py::STATEMENTS_PER_FUNCTION,
            methods_per_class: py::METHODS_PER_CLASS,
            statements_per_file: py::STATEMENTS_PER_FILE,
            functions_per_file: py::FUNCTIONS_PER_FILE,
            arguments_per_function: py::ARGUMENTS_PER_FUNCTION,
            arguments_positional: py::POSITIONAL_ARGS,
            arguments_keyword_only: py::KEYWORD_ONLY_ARGS,
            max_indentation_depth: py::MAX_INDENTATION,
            interface_types_per_file: py::INTERFACE_TYPES_PER_FILE,
            concrete_types_per_file: py::CONCRETE_TYPES_PER_FILE,
            nested_function_depth: py::NESTED_FUNCTION_DEPTH,
            returns_per_function: py::RETURNS_PER_FUNCTION,
            return_values_per_function: py::RETURN_VALUES_PER_FUNCTION,
            branches_per_function: py::BRANCHES_PER_FUNCTION,
            local_variables_per_function: py::LOCAL_VARIABLES,
            imported_names_per_file: py::IMPORTS_PER_FILE,
            statements_per_try_block: py::STATEMENTS_PER_TRY_BLOCK,
            boolean_parameters: py::BOOLEAN_PARAMETERS,
            annotations_per_function: py::DECORATORS_PER_FUNCTION,
            calls_per_function: py::CALLS_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: py::TRANSITIVE_DEPENDENCIES,
            dependency_depth: py::DEPENDENCY_DEPTH,
        }
    }

    pub const fn rust_defaults() -> Self {
        use defaults::{NOT_APPLICABLE as NA, rust as rs};
        Self {
            statements_per_function: rs::STATEMENTS_PER_FUNCTION,
            methods_per_class: rs::METHODS_PER_TYPE,
            statements_per_file: rs::STATEMENTS_PER_FILE,
            functions_per_file: rs::FUNCTIONS_PER_FILE,
            arguments_per_function: rs::ARGUMENTS,
            arguments_positional: NA,
            arguments_keyword_only: NA,
            max_indentation_depth: rs::MAX_INDENTATION,
            interface_types_per_file: rs::INTERFACE_TYPES_PER_FILE,
            concrete_types_per_file: rs::CONCRETE_TYPES_PER_FILE,
            nested_function_depth: rs::NESTED_FUNCTION_DEPTH,
            returns_per_function: rs::RETURNS_PER_FUNCTION,
            return_values_per_function: NA,
            branches_per_function: rs::BRANCHES_PER_FUNCTION,
            local_variables_per_function: rs::LOCAL_VARIABLES,
            imported_names_per_file: rs::IMPORTS_PER_FILE,
            statements_per_try_block: NA,
            boolean_parameters: rs::BOOLEAN_PARAMETERS,
            annotations_per_function: rs::ATTRIBUTES_PER_FUNCTION,
            calls_per_function: rs::CALLS_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: rs::TRANSITIVE_DEPENDENCIES,
            dependency_depth: rs::DEPENDENCY_DEPTH,
        }
    }

    fn load_config_chain(base: Self, lang: Option<ConfigLanguage>) -> Self {
        let mut config = base;
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(content) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
            config.merge_from_toml(&content, lang);
        }
        if let Ok(content) = std::fs::read_to_string(".kissconfig") {
            config.merge_from_toml(&content, lang);
        }
        config
    }

    pub fn load() -> Self {
        Self::load_config_chain(Self::default(), None)
    }

    pub fn load_for_language(lang: ConfigLanguage) -> Self {
        let base = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        Self::load_config_chain(base, Some(lang))
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml(&content, None);
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }
        config
    }

    pub fn load_from_for_language(path: &Path, lang: ConfigLanguage) -> Self {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml_with_path(&content, Some(lang), Some(path));
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }
        config
    }

    pub fn load_from_content(content: &str, lang: ConfigLanguage) -> Self {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        config.merge_from_toml(content, Some(lang));
        config
    }

    /// Try to load config from a file, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike `load_from`,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from(path: &Path, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::try_load_from_content(&content, lang)
    }

    /// Try to load config from TOML content, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike `load_from_content`,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from_content(content: &str, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        config.try_merge_from_toml(content, Some(lang))?;
        Ok(config)
    }

    fn merge_from_toml(&mut self, content: &str, lang: Option<ConfigLanguage>) {
        self.merge_from_toml_with_path(content, lang, None);
    }

    /// Result-based merge that returns errors instead of printing to stderr.
    fn try_merge_from_toml(
        &mut self,
        content: &str,
        lang: Option<ConfigLanguage>,
    ) -> Result<(), ConfigError> {
        let table = content.parse::<toml::Table>().map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })?;
        check_unknown_sections(&table)?;
        validate_config_keys(&table, lang)?;
        // All validations passed, apply using the regular merge (which won't print errors)
        self.merge_from_toml_with_path(content, lang, None);
        Ok(())
    }

    fn merge_from_toml_with_path(
        &mut self,
        content: &str,
        lang: Option<ConfigLanguage>,
        path: Option<&Path>,
    ) {
        let table = match content.parse::<toml::Table>() {
            Ok(t) => t,
            Err(e) => {
                if let Some(p) = path {
                    eprintln!("Warning: Failed to parse config {}: {}", p.display(), e);
                }
                return;
            }
        };
        if let Err(e) = check_unknown_sections(&table) {
            eprintln!("Error: {e}");
            return;
        }
        if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) {
            apply_thresholds(self, t);
        }
        if let Some(t) = table.get("shared").and_then(|v| v.as_table()) {
            apply_shared(self, t);
        }
        match lang {
            Some(ConfigLanguage::Python) => {
                if let Some(t) = table.get("python").and_then(|v| v.as_table()) {
                    apply_python(self, t);
                }
            }
            Some(ConfigLanguage::Rust) => {
                if let Some(t) = table.get("rust").and_then(|v| v.as_table()) {
                    apply_rust(self, t);
                }
            }
            None => {
                if let Some(t) = table.get("python").and_then(|v| v.as_table()) {
                    apply_python(self, t);
                }
                if let Some(t) = table.get("rust").and_then(|v| v.as_table()) {
                    apply_rust(self, t);
                }
            }
        }
    }

}

fn apply_thresholds(config: &mut Config, table: &toml::Table) {
    const VALID: &[&str] = &[
        "statements_per_function",
        "methods_per_class",
        "statements_per_file",
        "functions_per_file",
        "arguments_per_function",
        "arguments_positional",
        "arguments_keyword_only",
        "max_indentation_depth",
        "interface_types_per_file",
        "concrete_types_per_file",
        // Back-compat: older configs used classes_per_file for types-per-file.
        "classes_per_file",
        "nested_function_depth",
        "returns_per_function",
        "branches_per_function",
        "local_variables_per_function",
        "imported_names_per_file",
    ];
    if let Err(e) = check_unknown_keys(table, VALID, "thresholds") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_function" => statements_per_function, "methods_per_class" => methods_per_class,
        "statements_per_file" => statements_per_file, "functions_per_file" => functions_per_file,
        "arguments_per_function" => arguments_per_function,
        "arguments_positional" => arguments_positional, "arguments_keyword_only" => arguments_keyword_only,
        "max_indentation_depth" => max_indentation_depth,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "classes_per_file" => concrete_types_per_file,
        "nested_function_depth" => nested_function_depth, "returns_per_function" => returns_per_function,
        "branches_per_function" => branches_per_function, "local_variables_per_function" => local_variables_per_function,
        "imported_names_per_file" => imported_names_per_file);
}

fn apply_shared(config: &mut Config, table: &toml::Table) {
    const VALID: &[&str] = &[
        "statements_per_file",
        "functions_per_file",
        "interface_types_per_file",
        "concrete_types_per_file",
        // Back-compat: older configs used types_per_file for concrete types.
        "types_per_file",
        "imported_names_per_file",
        "cycle_size",
        "transitive_dependencies",
        "dependency_depth",
    ];
    if let Err(e) = check_unknown_keys(table, VALID, "shared") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_file" => statements_per_file,
        "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "imported_names_per_file" => imported_names_per_file,
        "cycle_size" => cycle_size,
        "transitive_dependencies" => transitive_dependencies,
        "dependency_depth" => dependency_depth);
}

fn apply_python(config: &mut Config, table: &toml::Table) {
    const VALID: &[&str] = &[
        "statements_per_function",
        "positional_args",
        "keyword_only_args",
        "max_indentation",
        "branches_per_function",
        "local_variables",
        "methods_per_class",
        "returns_per_function",
        "return_values_per_function",
        "nested_function_depth",
        "statements_per_try_block",
        "boolean_parameters",
        "decorators_per_function",
        "calls_per_function",
        "imported_names_per_file",
        "statements_per_file",
        "functions_per_file",
        "interface_types_per_file",
        "concrete_types_per_file",
        // Back-compat alias.
        "types_per_file",
        "cycle_size",
        "transitive_dependencies",
        "dependency_depth",
    ];
    if let Err(e) = check_unknown_keys(table, VALID, "python") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_function" => statements_per_function, "positional_args" => arguments_positional,
        "keyword_only_args" => arguments_keyword_only, "max_indentation" => max_indentation_depth,
        "branches_per_function" => branches_per_function, "local_variables" => local_variables_per_function,
        "methods_per_class" => methods_per_class, "returns_per_function" => returns_per_function,
        "return_values_per_function" => return_values_per_function, "nested_function_depth" => nested_function_depth,
        "statements_per_try_block" => statements_per_try_block, "boolean_parameters" => boolean_parameters,
        "decorators_per_function" => annotations_per_function, "calls_per_function" => calls_per_function,
        "imported_names_per_file" => imported_names_per_file,
        "statements_per_file" => statements_per_file, "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
}

fn apply_rust(config: &mut Config, table: &toml::Table) {
    const VALID: &[&str] = &[
        "statements_per_function",
        "arguments",
        "max_indentation",
        "branches_per_function",
        "local_variables",
        "methods_per_class",
        "statements_per_file",
        "functions_per_file",
        "interface_types_per_file",
        "concrete_types_per_file",
        // Back-compat alias.
        "types_per_file",
        "returns_per_function",
        "nested_function_depth",
        "boolean_parameters",
        "attributes_per_function",
        "calls_per_function",
        "imported_names_per_file",
        "cycle_size",
        "transitive_dependencies",
        "dependency_depth",
        "nested_closure_depth",
    ];
    if let Err(e) = check_unknown_keys(table, VALID, "rust") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_function" => statements_per_function, "arguments" => arguments_per_function,
        "max_indentation" => max_indentation_depth, "branches_per_function" => branches_per_function,
        "local_variables" => local_variables_per_function, "methods_per_class" => methods_per_class,
        "statements_per_file" => statements_per_file, "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "returns_per_function" => returns_per_function,
        "nested_function_depth" => nested_function_depth, "boolean_parameters" => boolean_parameters,
        "attributes_per_function" => annotations_per_function, "calls_per_function" => calls_per_function,
        "imported_names_per_file" => imported_names_per_file,
        "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
}

pub(crate) fn check_unknown_keys(
    table: &toml::Table,
    valid: &[&str],
    section: &str,
) -> Result<(), ConfigError> {
    for key in table.keys() {
        if !valid.contains(&key.as_str()) {
            return Err(ConfigError::UnknownKey {
                key: key.clone(),
                section: section.to_string(),
            });
        }
    }
    Ok(())
}

fn check_unknown_sections(table: &toml::Table) -> Result<(), ConfigError> {
    const VALID: &[&str] = &["python", "rust", "shared", "thresholds", "gate"];
    for key in table.keys() {
        if VALID.contains(&key.as_str()) {
            continue;
        }
        let hint = VALID.iter().find(|v| similar(key, v)).map(|s| (*s).to_string());
        return Err(ConfigError::UnknownSection { section: key.clone(), hint });
    }
    Ok(())
}

// Validation functions for try_merge_from_toml
fn validate_config_keys(table: &toml::Table, lang: Option<ConfigLanguage>) -> Result<(), ConfigError> {
    if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) {
        validate_thresholds_keys(t)?;
    }
    if let Some(t) = table.get("shared").and_then(|v| v.as_table()) {
        validate_shared_keys(t)?;
    }
    let check_py = lang.is_none() || matches!(lang, Some(ConfigLanguage::Python));
    let check_rs = lang.is_none() || matches!(lang, Some(ConfigLanguage::Rust));
    if check_py
        && let Some(t) = table.get("python").and_then(|v| v.as_table())
    {
        validate_python_keys(t)?;
    }
    if check_rs
        && let Some(t) = table.get("rust").and_then(|v| v.as_table())
    {
        validate_rust_keys(t)?;
    }
    Ok(())
}

fn validate_thresholds_keys(table: &toml::Table) -> Result<(), ConfigError> {
    const VALID: &[&str] = &[
        "statements_per_function", "methods_per_class", "statements_per_file",
        "functions_per_file", "arguments_per_function", "arguments_positional",
        "arguments_keyword_only", "max_indentation_depth", "interface_types_per_file",
        "concrete_types_per_file", "classes_per_file", "nested_function_depth",
        "returns_per_function", "branches_per_function", "local_variables_per_function",
        "imported_names_per_file",
    ];
    check_unknown_keys(table, VALID, "thresholds")
}

fn validate_shared_keys(table: &toml::Table) -> Result<(), ConfigError> {
    const VALID: &[&str] = &[
        "statements_per_file", "functions_per_file", "interface_types_per_file",
        "concrete_types_per_file", "types_per_file", "imported_names_per_file",
        "cycle_size", "transitive_dependencies", "dependency_depth",
    ];
    check_unknown_keys(table, VALID, "shared")
}

fn validate_python_keys(table: &toml::Table) -> Result<(), ConfigError> {
    const VALID: &[&str] = &[
        "statements_per_function", "positional_args", "keyword_only_args", "max_indentation",
        "branches_per_function", "local_variables", "methods_per_class", "returns_per_function",
        "return_values_per_function", "nested_function_depth", "statements_per_try_block",
        "boolean_parameters", "decorators_per_function", "calls_per_function",
        "imported_names_per_file", "statements_per_file", "functions_per_file",
        "interface_types_per_file", "concrete_types_per_file", "types_per_file",
        "cycle_size", "transitive_dependencies", "dependency_depth",
    ];
    check_unknown_keys(table, VALID, "python")
}

fn validate_rust_keys(table: &toml::Table) -> Result<(), ConfigError> {
    const VALID: &[&str] = &[
        "statements_per_function", "arguments", "max_indentation", "branches_per_function",
        "local_variables", "methods_per_class", "statements_per_file", "functions_per_file",
        "interface_types_per_file", "concrete_types_per_file", "types_per_file",
        "returns_per_function", "nested_function_depth", "boolean_parameters",
        "attributes_per_function", "calls_per_function", "imported_names_per_file",
        "cycle_size", "transitive_dependencies", "dependency_depth", "nested_closure_depth",
    ];
    check_unknown_keys(table, VALID, "rust")
}

fn similar(a: &str, b: &str) -> bool {
    if a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    let common = a.chars().filter(|c| b.contains(*c)).count();
    common >= a.len().saturating_sub(2) && common >= b.len().saturating_sub(2)
}

pub(crate) fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    let value = table.get(key)?;
    if let Some(v) = value.as_integer() {
        if v < 0 {
            eprintln!("Warning: Config key '{key}' must be non-negative, got {v}");
            return None;
        }
        return usize::try_from(v).ok();
    }
    eprintln!(
        "Warning: Config key '{key}' expected integer, got {}",
        value.type_str()
    );
    None
}

pub fn is_similar(a: &str, b: &str) -> bool {
    similar(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(rs.arguments_per_function, 5);

        let mut c = Config::python_defaults();
        let mut t3 = toml::Table::new();
        t3.insert("statements_per_file".into(), toml::Value::Integer(999));
        apply_shared(&mut c, &t3);
        assert_eq!(c.statements_per_file, 999);
    }

    #[test]
    fn test_helpers() {
        assert!(
            similar("python", "pytohn") && similar("rust", "ruts") && !similar("python", "xyz")
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
    fn test_unknown_section_returns_error() {
        let mut t = toml::Table::new();
        t.insert("unknown_section".into(), toml::Value::Table(toml::Table::new()));
        let result = check_unknown_sections(&t);
        assert!(result.is_err());
    }
}
