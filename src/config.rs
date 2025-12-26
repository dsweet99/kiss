
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
            lines_per_file: defaults::python::LINES_PER_FILE,
            arguments_per_function: 7,
            arguments_positional: defaults::python::POSITIONAL_ARGS,
            arguments_keyword_only: defaults::python::KEYWORD_ONLY_ARGS,
            max_indentation_depth: defaults::python::MAX_INDENTATION,
            classes_per_file: defaults::python::TYPES_PER_FILE,
            nested_function_depth: defaults::python::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::python::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::python::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::python::LOCAL_VARIABLES,
            imports_per_file: defaults::python::IMPORTS_PER_FILE,
            statements_per_try_block: defaults::python::STATEMENTS_PER_TRY_BLOCK,
            boolean_parameters: defaults::python::BOOLEAN_PARAMETERS,
            decorators_per_function: defaults::python::DECORATORS_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: defaults::graph::TRANSITIVE_DEPENDENCIES,
            dependency_depth: defaults::graph::DEPENDENCY_DEPTH,
        }
    }

    pub const fn rust_defaults() -> Self {
        Self {
            statements_per_function: defaults::rust::STATEMENTS_PER_FUNCTION,
            methods_per_class: defaults::rust::METHODS_PER_TYPE,
            lines_per_file: defaults::rust::LINES_PER_FILE,
            arguments_per_function: defaults::rust::ARGUMENTS,
            arguments_positional: 5,
            arguments_keyword_only: 6,
            max_indentation_depth: defaults::rust::MAX_INDENTATION,
            classes_per_file: defaults::rust::TYPES_PER_FILE,
            nested_function_depth: defaults::rust::NESTED_FUNCTION_DEPTH,
            returns_per_function: defaults::rust::RETURNS_PER_FUNCTION,
            branches_per_function: defaults::rust::BRANCHES_PER_FUNCTION,
            local_variables_per_function: defaults::rust::LOCAL_VARIABLES,
            imports_per_file: defaults::rust::IMPORTS_PER_FILE,
            statements_per_try_block: usize::MAX,
            boolean_parameters: defaults::rust::BOOLEAN_PARAMETERS,
            decorators_per_function: defaults::rust::ATTRIBUTES_PER_FUNCTION,
            cycle_size: defaults::graph::CYCLE_SIZE,
            transitive_dependencies: defaults::graph::TRANSITIVE_DEPENDENCIES,
            dependency_depth: defaults::graph::DEPENDENCY_DEPTH,
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
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "methods_per_class" => methods_per_class,
            "lines_per_file" => lines_per_file, "arguments_per_function" => arguments_per_function,
            "arguments_positional" => arguments_positional, "arguments_keyword_only" => arguments_keyword_only,
            "max_indentation_depth" => max_indentation_depth, "classes_per_file" => classes_per_file,
            "nested_function_depth" => nested_function_depth, "returns_per_function" => returns_per_function,
            "branches_per_function" => branches_per_function, "local_variables_per_function" => local_variables_per_function,
            "imports_per_file" => imports_per_file);
    }

    fn apply_shared(&mut self, table: &toml::Table) {
        apply_config!(self, table, "lines_per_file" => lines_per_file, "types_per_file" => classes_per_file, 
            "imports_per_file" => imports_per_file, "cycle_size" => cycle_size,
            "transitive_dependencies" => transitive_dependencies, "dependency_depth" => dependency_depth);
    }

    fn apply_python(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "positional_args" => arguments_positional,
            "keyword_only_args" => arguments_keyword_only, "max_indentation" => max_indentation_depth,
            "branches_per_function" => branches_per_function, "local_variables" => local_variables_per_function,
            "methods_per_class" => methods_per_class, "returns_per_function" => returns_per_function, "nested_function_depth" => nested_function_depth,
            "statements_per_try_block" => statements_per_try_block, "boolean_parameters" => boolean_parameters,
            "decorators_per_function" => decorators_per_function);
    }

    fn apply_rust(&mut self, table: &toml::Table) {
        apply_config!(self, table,
            "statements_per_function" => statements_per_function, "arguments" => arguments_per_function,
            "max_indentation" => max_indentation_depth, "branches_per_function" => branches_per_function,
            "local_variables" => local_variables_per_function, "methods_per_type" => methods_per_class,
            "lines_per_file" => lines_per_file,
            "types_per_file" => classes_per_file, "returns_per_function" => returns_per_function,
            "nested_function_depth" => nested_function_depth, "bool_parameters" => boolean_parameters,
            "attributes_per_function" => decorators_per_function);
    }
}

fn get_usize(table: &toml::Table, key: &str) -> Option<usize> {
    table.get(key).and_then(toml::Value::as_integer).and_then(|v| usize::try_from(v).ok())
}

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize,
    pub min_similarity: f64,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD,
            min_similarity: defaults::duplication::MIN_SIMILARITY,
        }
    }
}

impl GateConfig {
    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(content) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
            config.merge_from_toml(&content);
        }
        if let Ok(content) = std::fs::read_to_string(".kissconfig") { config.merge_from_toml(&content); }
        config
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(content) = std::fs::read_to_string(path) { config.merge_from_toml(&content); }
        config
    }

    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else { return };
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            if let Some(thresh) = get_usize(gate, "test_coverage_threshold") {
                self.test_coverage_threshold = thresh.min(100);
            }
            if let Some(sim) = get_f64(gate, "min_similarity") {
                self.min_similarity = sim.clamp(0.0, 1.0);
            }
        }
    }
}

fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    table.get(key).and_then(toml::Value::as_float)
}
