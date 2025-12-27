
use crate::defaults;
use std::path::Path;

macro_rules! apply_config {
    ($self:ident, $table:ident, $($key:literal => $field:ident),+ $(,)?) => {
        $( if let Some(v) = get_usize($table, $key) { $self.$field = v; } )+
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLanguage { Python, Rust }

#[derive(Debug, Clone)]
pub struct Config {
    pub statements_per_function: usize,
    pub methods_per_class: usize,
    pub statements_per_file: usize,
    pub arguments_per_function: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation_depth: usize,
    pub classes_per_file: usize,
    pub nested_function_depth: usize,
    pub returns_per_function: usize,
    pub branches_per_function: usize,
    pub local_variables_per_function: usize,
    pub imported_names_per_file: usize,
    pub statements_per_try_block: usize,
    pub boolean_parameters: usize,
    pub decorators_per_function: usize,
    pub cycle_size: usize,
    pub transitive_dependencies: usize,
    pub dependency_depth: usize,
}

impl Default for Config {
    fn default() -> Self { Self::python_defaults() }
}

impl Config {
    pub const fn python_defaults() -> Self {
        Self {
            statements_per_function: defaults::python::STATEMENTS_PER_FUNCTION,
            methods_per_class: defaults::python::METHODS_PER_CLASS,
            statements_per_file: defaults::python::STATEMENTS_PER_FILE,
            arguments_per_function: defaults::python::ARGUMENTS_PER_FUNCTION,
            arguments_positional: defaults::python::POSITIONAL_ARGS,
            arguments_keyword_only: defaults::python::KEYWORD_ONLY_ARGS,
            max_indentation_depth: defaults::python::MAX_INDENTATION,
            classes_per_file: defaults::python::TYPES_PER_FILE,
            nested_function_depth: defaults::python::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::python::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::python::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::python::LOCAL_VARIABLES,
            imported_names_per_file: defaults::python::IMPORTS_PER_FILE,
            statements_per_try_block: defaults::python::STATEMENTS_PER_TRY_BLOCK,
            boolean_parameters: defaults::python::BOOLEAN_PARAMETERS,
            decorators_per_function: defaults::python::DECORATORS_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: defaults::python::TRANSITIVE_DEPENDENCIES,
            dependency_depth: defaults::python::DEPENDENCY_DEPTH,
        }
    }

    pub const fn rust_defaults() -> Self {
        Self {
            statements_per_function: defaults::rust::STATEMENTS_PER_FUNCTION,
            methods_per_class: defaults::rust::METHODS_PER_TYPE,
            statements_per_file: defaults::rust::STATEMENTS_PER_FILE,
            arguments_per_function: defaults::rust::ARGUMENTS,
            arguments_positional: 5,
            arguments_keyword_only: 6,
            max_indentation_depth: defaults::rust::MAX_INDENTATION,
            classes_per_file: defaults::rust::TYPES_PER_FILE,
            nested_function_depth: defaults::rust::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::rust::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::rust::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::rust::LOCAL_VARIABLES,
            imported_names_per_file: defaults::rust::IMPORTS_PER_FILE,
            statements_per_try_block: usize::MAX,
            boolean_parameters: defaults::rust::BOOLEAN_PARAMETERS,
            decorators_per_function: defaults::rust::ATTRIBUTES_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: defaults::rust::TRANSITIVE_DEPENDENCIES,
            dependency_depth: defaults::rust::DEPENDENCY_DEPTH,
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

    pub fn load() -> Self { Self::load_config_chain(Self::default(), None) }

    pub fn load_for_language(lang: ConfigLanguage) -> Self {
        let base = match lang { ConfigLanguage::Python => Self::python_defaults(), ConfigLanguage::Rust => Self::rust_defaults() };
        Self::load_config_chain(base, Some(lang))
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(content) = std::fs::read_to_string(path) { config.merge_from_toml(&content, None); }
        else { eprintln!("Warning: Could not read config file: {}", path.display()); }
        config
    }

    pub fn load_from_for_language(path: &Path, lang: ConfigLanguage) -> Self {
        let mut config = match lang { ConfigLanguage::Python => Self::python_defaults(), ConfigLanguage::Rust => Self::rust_defaults() };
        if let Ok(content) = std::fs::read_to_string(path) { config.merge_from_toml_with_path(&content, Some(lang), Some(path)); }
        else { eprintln!("Warning: Could not read config file: {}", path.display()); }
        config
    }

    pub fn load_from_content(content: &str, lang: ConfigLanguage) -> Self {
        let mut config = match lang { ConfigLanguage::Python => Self::python_defaults(), ConfigLanguage::Rust => Self::rust_defaults() };
        config.merge_from_toml(content, Some(lang));
        config
    }

    fn merge_from_toml(&mut self, content: &str, lang: Option<ConfigLanguage>) {
        self.merge_from_toml_with_path(content, lang, None);
    }

    fn merge_from_toml_with_path(&mut self, content: &str, lang: Option<ConfigLanguage>, path: Option<&Path>) {
        let table = match content.parse::<toml::Table>() {
            Ok(t) => t,
            Err(e) => {
                if let Some(p) = path {
                    eprintln!("Warning: Failed to parse config {}: {}", p.display(), e);
                }
                return;
            }
        };
        check_unknown_sections(&table);
        if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) { self.apply_thresholds(t); }
        if let Some(t) = table.get("shared").and_then(|v| v.as_table()) { self.apply_shared(t); }
        match lang {
            Some(ConfigLanguage::Python) => if let Some(t) = table.get("python").and_then(|v| v.as_table()) { self.apply_python(t); },
            Some(ConfigLanguage::Rust) => if let Some(t) = table.get("rust").and_then(|v| v.as_table()) { self.apply_rust(t); },
            None => {
                if let Some(t) = table.get("python").and_then(|v| v.as_table()) { self.apply_python(t); }
                if let Some(t) = table.get("rust").and_then(|v| v.as_table()) { self.apply_rust(t); }
            }
        }
    }

    fn apply_thresholds(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_function", "methods_per_class", "statements_per_file",
            "arguments_per_function", "arguments_positional", "arguments_keyword_only",
            "max_indentation_depth", "classes_per_file", "nested_function_depth", "returns_per_function",
            "branches_per_function", "local_variables_per_function", "imported_names_per_file"];
        check_unknown_keys(table, VALID, "thresholds");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "methods_per_class" => methods_per_class,
            "statements_per_file" => statements_per_file, "arguments_per_function" => arguments_per_function,
            "arguments_positional" => arguments_positional, "arguments_keyword_only" => arguments_keyword_only,
            "max_indentation_depth" => max_indentation_depth, "classes_per_file" => classes_per_file,
            "nested_function_depth" => nested_function_depth, "returns_per_function" => returns_per_function,
            "branches_per_function" => branches_per_function, "local_variables_per_function" => local_variables_per_function,
            "imported_names_per_file" => imported_names_per_file);
    }

    fn apply_shared(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_file", "types_per_file", "imported_names_per_file",
            "cycle_size", "transitive_dependencies", "dependency_depth"];
        check_unknown_keys(table, VALID, "shared");
        apply_config!(self, table, "statements_per_file" => statements_per_file, "types_per_file" => classes_per_file, 
            "imported_names_per_file" => imported_names_per_file, "cycle_size" => cycle_size,
            "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }

    fn apply_python(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_function", "positional_args", "keyword_only_args",
            "max_indentation", "branches_per_function", "local_variables", "methods_per_class",
            "returns_per_function", "nested_function_depth", "statements_per_try_block",
            "boolean_parameters", "decorators_per_function", "imported_names_per_file", "statements_per_file",
            "types_per_file", "cycle_size", "transitive_dependencies", "dependency_depth"];
        check_unknown_keys(table, VALID, "python");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "positional_args" => arguments_positional,
            "keyword_only_args" => arguments_keyword_only, "max_indentation" => max_indentation_depth,
            "branches_per_function" => branches_per_function, "local_variables" => local_variables_per_function,
            "methods_per_class" => methods_per_class, "returns_per_function" => returns_per_function, "nested_function_depth" => nested_function_depth,
            "statements_per_try_block" => statements_per_try_block, "boolean_parameters" => boolean_parameters,
            "decorators_per_function" => decorators_per_function, "statements_per_file" => statements_per_file,
            "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }

    fn apply_rust(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_function", "arguments", "max_indentation",
            "branches_per_function", "local_variables", "methods_per_class", "statements_per_file",
            "types_per_file", "returns_per_function", "nested_function_depth",
            "boolean_parameters", "attributes_per_function", "imported_names_per_file",
            "cycle_size", "transitive_dependencies", "dependency_depth", "nested_closure_depth"];
        check_unknown_keys(table, VALID, "rust");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "arguments" => arguments_per_function,
            "max_indentation" => max_indentation_depth, "branches_per_function" => branches_per_function,
            "local_variables" => local_variables_per_function, "methods_per_class" => methods_per_class,
            "statements_per_file" => statements_per_file,
            "types_per_file" => classes_per_file, "returns_per_function" => returns_per_function,
            "nested_function_depth" => nested_function_depth, "boolean_parameters" => boolean_parameters,
            "attributes_per_function" => decorators_per_function,
            "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }
}

fn check_unknown_keys(table: &toml::Table, valid: &[&str], section: &str) {
    for key in table.keys() {
        if !valid.contains(&key.as_str()) {
            eprintln!("Error: Unknown config key '{key}' in [{section}]");
            std::process::exit(1);
        }
    }
}

fn check_unknown_sections(table: &toml::Table) {
    const VALID: &[&str] = &["python", "rust", "shared", "thresholds", "gate"];
    for key in table.keys() {
        if VALID.contains(&key.as_str()) { continue; }
        let hint = VALID.iter().find(|v| similar(key, v)).map(|s| format!(" - did you mean '[{s}]'?")).unwrap_or_default();
        eprintln!("Error: Unknown config section '[{key}]'{hint}"); std::process::exit(1);
    }
}

fn similar(a: &str, b: &str) -> bool {
    if a.len().abs_diff(b.len()) > 2 { return false; }
    let common = a.chars().filter(|c| b.contains(*c)).count();
    common >= a.len().saturating_sub(2) && common >= b.len().saturating_sub(2)
}

fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    let value = table.get(key)?;
    if let Some(v) = value.as_integer() {
        if v < 0 {
            eprintln!("Warning: Config key '{key}' must be non-negative, got {v}");
            return None;
        }
        return usize::try_from(v).ok();
    }
    eprintln!("Warning: Config key '{key}' expected integer, got {}", value.type_str());
    None
}

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize,
    pub min_similarity: f64,
}

impl Default for GateConfig {
    fn default() -> Self { Self { test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD, min_similarity: defaults::duplication::MIN_SIMILARITY } }
}

impl GateConfig {
    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(home) = std::env::var_os("HOME") && let Ok(c) = std::fs::read_to_string(Path::new(&home).join(".kissconfig")) { config.merge_from_toml(&c); }
        if let Ok(c) = std::fs::read_to_string(".kissconfig") { config.merge_from_toml(&c); }
        config
    }
    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(c) = std::fs::read_to_string(path) { config.merge_from_toml(&c); }
        config
    }
    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else { return };
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            check_unknown_keys(gate, &["test_coverage_threshold", "min_similarity"], "gate");
            if let Some(t) = get_usize(gate, "test_coverage_threshold") {
                if t > 100 { eprintln!("Error: test_coverage_threshold must be 0-100, got {t}"); std::process::exit(1); }
                self.test_coverage_threshold = t;
            }
            if let Some(s) = get_f64(gate, "min_similarity") {
                if !(0.0..=1.0).contains(&s) { eprintln!("Error: min_similarity must be 0.0-1.0, got {s}"); std::process::exit(1); }
                self.min_similarity = s;
            }
        }
    }
}

fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value.as_float().or_else(|| {
        eprintln!("Warning: Config key '{key}' expected float, got {}", value.type_str());
        None
    })
}

pub fn is_similar(a: &str, b: &str) -> bool { similar(a, b) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_config_chain() {
        // Test with no config files - just verify it doesn't panic
        let config = Config::load_config_chain(Config::python_defaults(), Some(ConfigLanguage::Python));
        assert!(config.statements_per_function > 0);
    }

    #[test]
    fn test_merge_from_toml() {
        let mut config = Config::python_defaults();
        config.merge_from_toml("[python]\nstatements_per_function = 99", Some(ConfigLanguage::Python));
        assert_eq!(config.statements_per_function, 99);
    }

    #[test]
    fn test_merge_from_toml_with_path() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");
        std::fs::write(&path, "[python]\nstatements_per_function = 88").unwrap();
        let mut config = Config::python_defaults();
        let content = std::fs::read_to_string(&path).unwrap();
        config.merge_from_toml_with_path(&content, Some(ConfigLanguage::Python), Some(&path));
        assert_eq!(config.statements_per_function, 88);
    }

    #[test]
    fn test_apply_thresholds() {
        let mut config = Config::python_defaults();
        let mut table = toml::Table::new();
        table.insert("statements_per_function".into(), toml::Value::Integer(42));
        config.apply_thresholds(&table);
        assert_eq!(config.statements_per_function, 42);
    }

    #[test]
    fn test_apply_shared() {
        let mut config = Config::python_defaults();
        let mut table = toml::Table::new();
        table.insert("statements_per_file".into(), toml::Value::Integer(999));
        config.apply_shared(&table);
        assert_eq!(config.statements_per_file, 999);
    }

    #[test]
    fn test_apply_python() {
        let mut config = Config::python_defaults();
        let mut table = toml::Table::new();
        table.insert("positional_args".into(), toml::Value::Integer(3));
        config.apply_python(&table);
        assert_eq!(config.arguments_positional, 3);
    }

    #[test]
    fn test_apply_rust() {
        let mut config = Config::rust_defaults();
        let mut table = toml::Table::new();
        table.insert("arguments".into(), toml::Value::Integer(5));
        config.apply_rust(&table);
        assert_eq!(config.arguments_per_function, 5);
    }

    #[test]
    fn test_similar() {
        assert!(similar("python", "pytohn"));
        assert!(similar("rust", "ruts"));
        assert!(!similar("python", "xyz"));
    }

    #[test]
    fn test_get_usize() {
        let mut table = toml::Table::new();
        table.insert("valid".into(), toml::Value::Integer(42));
        assert_eq!(get_usize(&table, "valid"), Some(42));
        assert_eq!(get_usize(&table, "missing"), None);
        table.insert("negative".into(), toml::Value::Integer(-1));
        assert_eq!(get_usize(&table, "negative"), None);
        table.insert("wrong_type".into(), toml::Value::String("hi".into()));
        assert_eq!(get_usize(&table, "wrong_type"), None);
    }

    #[test]
    fn test_get_f64() {
        let mut table = toml::Table::new();
        table.insert("valid".into(), toml::Value::Float(0.5));
        assert_eq!(get_f64(&table, "valid"), Some(0.5));
        assert_eq!(get_f64(&table, "missing"), None);
        table.insert("wrong_type".into(), toml::Value::Integer(1));
        assert_eq!(get_f64(&table, "wrong_type"), None);
    }

    #[test]
    fn test_gate_config_merge_from_toml() {
        let mut gate = GateConfig::default();
        gate.merge_from_toml("[gate]\ntest_coverage_threshold = 50\nmin_similarity = 0.8");
        assert_eq!(gate.test_coverage_threshold, 50);
        assert!((gate.min_similarity - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_check_unknown_keys_valid() {
        let mut table = toml::Table::new();
        table.insert("statements_per_function".into(), toml::Value::Integer(30));
        // Should not panic for valid keys
        check_unknown_keys(&table, &["statements_per_function"], "test");
    }

    #[test]
    fn test_check_unknown_sections_valid() {
        let mut table = toml::Table::new();
        table.insert("python".into(), toml::Value::Table(toml::Table::new()));
        table.insert("rust".into(), toml::Value::Table(toml::Table::new()));
        // Should not panic for valid sections
        check_unknown_sections(&table);
    }
}
