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
    pub cycle_size: usize,
    pub transitive_dependencies: usize,
    pub dependency_depth: usize,
}

impl Default for Config {
    fn default() -> Self { Self::python_defaults() }
}

impl Config {
    pub const fn python_defaults() -> Self {
        use defaults::python as py;
        Self {
            statements_per_function: py::STATEMENTS_PER_FUNCTION, methods_per_class: py::METHODS_PER_CLASS,
            statements_per_file: py::STATEMENTS_PER_FILE, arguments_per_function: py::ARGUMENTS_PER_FUNCTION,
            arguments_positional: py::POSITIONAL_ARGS, arguments_keyword_only: py::KEYWORD_ONLY_ARGS,
            max_indentation_depth: py::MAX_INDENTATION,
            interface_types_per_file: py::INTERFACE_TYPES_PER_FILE,
            concrete_types_per_file: py::CONCRETE_TYPES_PER_FILE,
            nested_function_depth: py::NESTED_FUNCTION_DEPTH, returns_per_function: py::RETURNS_PER_FUNCTION,
            return_values_per_function: py::RETURN_VALUES_PER_FUNCTION, branches_per_function: py::BRANCHES_PER_FUNCTION,
            local_variables_per_function: py::LOCAL_VARIABLES, imported_names_per_file: py::IMPORTS_PER_FILE,
            statements_per_try_block: py::STATEMENTS_PER_TRY_BLOCK, boolean_parameters: py::BOOLEAN_PARAMETERS,
            annotations_per_function: py::DECORATORS_PER_FUNCTION, cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: py::TRANSITIVE_DEPENDENCIES, dependency_depth: py::DEPENDENCY_DEPTH,
        }
    }

    pub const fn rust_defaults() -> Self {
        use defaults::{rust as rs, NOT_APPLICABLE as NA};
        Self {
            statements_per_function: rs::STATEMENTS_PER_FUNCTION, methods_per_class: rs::METHODS_PER_TYPE,
            statements_per_file: rs::STATEMENTS_PER_FILE, arguments_per_function: rs::ARGUMENTS,
            arguments_positional: NA, arguments_keyword_only: NA,
            max_indentation_depth: rs::MAX_INDENTATION,
            interface_types_per_file: rs::INTERFACE_TYPES_PER_FILE,
            concrete_types_per_file: rs::CONCRETE_TYPES_PER_FILE,
            nested_function_depth: rs::NESTED_FUNCTION_DEPTH, returns_per_function: rs::RETURNS_PER_FUNCTION,
            return_values_per_function: NA, branches_per_function: rs::BRANCHES_PER_FUNCTION,
            local_variables_per_function: rs::LOCAL_VARIABLES, imported_names_per_file: rs::IMPORTS_PER_FILE,
            statements_per_try_block: NA, boolean_parameters: rs::BOOLEAN_PARAMETERS,
            annotations_per_function: rs::ATTRIBUTES_PER_FUNCTION, cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: rs::TRANSITIVE_DEPENDENCIES, dependency_depth: rs::DEPENDENCY_DEPTH,
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
            "max_indentation_depth", "interface_types_per_file", "concrete_types_per_file",
            // Back-compat: older configs used classes_per_file for types-per-file.
            "classes_per_file",
            "nested_function_depth", "returns_per_function",
            "branches_per_function", "local_variables_per_function", "imported_names_per_file"];
        check_unknown_keys(table, VALID, "thresholds");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "methods_per_class" => methods_per_class,
            "statements_per_file" => statements_per_file, "arguments_per_function" => arguments_per_function,
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

    fn apply_shared(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_file", "interface_types_per_file", "concrete_types_per_file",
            // Back-compat: older configs used types_per_file for concrete types.
            "types_per_file",
            "imported_names_per_file",
            "cycle_size", "transitive_dependencies", "dependency_depth"];
        check_unknown_keys(table, VALID, "shared");
        apply_config!(self, table,
            "statements_per_file" => statements_per_file,
            "interface_types_per_file" => interface_types_per_file,
            "concrete_types_per_file" => concrete_types_per_file,
            // Back-compat alias.
            "types_per_file" => concrete_types_per_file,
            "imported_names_per_file" => imported_names_per_file,
            "cycle_size" => cycle_size,
            "transitive_dependencies" => transitive_dependencies,
            "dependency_depth" => dependency_depth);
    }

    fn apply_python(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_function", "positional_args", "keyword_only_args",
            "max_indentation", "branches_per_function", "local_variables", "methods_per_class",
            "returns_per_function", "return_values_per_function", "nested_function_depth", "statements_per_try_block",
            "boolean_parameters", "decorators_per_function", "imported_names_per_file", "statements_per_file",
            "interface_types_per_file", "concrete_types_per_file",
            // Back-compat alias.
            "types_per_file",
            "cycle_size", "transitive_dependencies", "dependency_depth"];
        check_unknown_keys(table, VALID, "python");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "positional_args" => arguments_positional,
            "keyword_only_args" => arguments_keyword_only, "max_indentation" => max_indentation_depth,
            "branches_per_function" => branches_per_function, "local_variables" => local_variables_per_function,
            "methods_per_class" => methods_per_class, "returns_per_function" => returns_per_function,
            "return_values_per_function" => return_values_per_function, "nested_function_depth" => nested_function_depth,
            "statements_per_try_block" => statements_per_try_block, "boolean_parameters" => boolean_parameters,
            "decorators_per_function" => annotations_per_function, "imported_names_per_file" => imported_names_per_file,
            "statements_per_file" => statements_per_file,
            "interface_types_per_file" => interface_types_per_file,
            "concrete_types_per_file" => concrete_types_per_file,
            // Back-compat alias.
            "types_per_file" => concrete_types_per_file,
            "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }

    fn apply_rust(&mut self, table: &toml::Table) {
        const VALID: &[&str] = &["statements_per_function", "arguments", "max_indentation",
            "branches_per_function", "local_variables", "methods_per_class", "statements_per_file",
            "interface_types_per_file", "concrete_types_per_file",
            // Back-compat alias.
            "types_per_file",
            "returns_per_function", "nested_function_depth",
            "boolean_parameters", "attributes_per_function", "imported_names_per_file",
            "cycle_size", "transitive_dependencies", "dependency_depth", "nested_closure_depth"];
        check_unknown_keys(table, VALID, "rust");
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "arguments" => arguments_per_function,
            "max_indentation" => max_indentation_depth, "branches_per_function" => branches_per_function,
            "local_variables" => local_variables_per_function, "methods_per_class" => methods_per_class,
            "statements_per_file" => statements_per_file,
            "interface_types_per_file" => interface_types_per_file,
            "concrete_types_per_file" => concrete_types_per_file,
            // Back-compat alias.
            "types_per_file" => concrete_types_per_file,
            "returns_per_function" => returns_per_function,
            "nested_function_depth" => nested_function_depth, "boolean_parameters" => boolean_parameters,
            "attributes_per_function" => annotations_per_function, "imported_names_per_file" => imported_names_per_file,
            "cycle_size" => cycle_size, "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }
}

pub(crate) fn check_unknown_keys(table: &toml::Table, valid: &[&str], section: &str) {
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

pub(crate) fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
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

pub fn is_similar(a: &str, b: &str) -> bool { similar(a, b) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_and_apply() {
        let mut c = Config::python_defaults();
        c.merge_from_toml("[python]\nstatements_per_function = 99", Some(ConfigLanguage::Python));
        assert_eq!(c.statements_per_function, 99);

        let mut table = toml::Table::new();
        table.insert("statements_per_function".into(), toml::Value::Integer(42));
        let mut c2 = Config::python_defaults();
        c2.apply_thresholds(&table);
        assert_eq!(c2.statements_per_function, 42);
    }

    #[test]
    fn test_apply_language_sections() {
        let mut py = Config::python_defaults();
        let mut t = toml::Table::new();
        t.insert("positional_args".into(), toml::Value::Integer(3));
        py.apply_python(&t);
        assert_eq!(py.arguments_positional, 3);

        let mut rs = Config::rust_defaults();
        let mut t2 = toml::Table::new();
        t2.insert("arguments".into(), toml::Value::Integer(5));
        rs.apply_rust(&t2);
        assert_eq!(rs.arguments_per_function, 5);

        let mut c = Config::python_defaults();
        let mut t3 = toml::Table::new();
        t3.insert("statements_per_file".into(), toml::Value::Integer(999));
        c.apply_shared(&t3);
        assert_eq!(c.statements_per_file, 999);
    }

    #[test]
    fn test_helpers() {
        assert!(similar("python", "pytohn") && similar("rust", "ruts") && !similar("python", "xyz"));
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
        check_unknown_keys(&t, &["statements_per_function"], "test");
        let mut t2 = toml::Table::new();
        t2.insert("python".into(), toml::Value::Table(toml::Table::new()));
        check_unknown_sections(&t2);
    }
}
