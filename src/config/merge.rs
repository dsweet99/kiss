use std::path::Path;

use crate::config::error::ConfigError;
use crate::config::keys::{PYTHON_KEYS, RUST_KEYS, SHARED_KEYS, THRESHOLDS_KEYS};
use crate::config::types::{Config, ConfigLanguage};
use crate::config::validation::{
    check_unknown_keys, check_unknown_sections, get_usize, validate_config_keys,
};

macro_rules! apply_config {
    ($self:ident, $table:ident, $($key:literal => $field:ident),+ $(,)?) => {
        $( if let Some(v) = get_usize($table, $key) { $self.$field = v; } )+
    };
}

pub(crate) fn apply_thresholds(config: &mut Config, table: &toml::Table) {
    if let Err(e) = check_unknown_keys(table, THRESHOLDS_KEYS, "thresholds") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_function" => statements_per_function, "methods_per_class" => methods_per_class,
        "statements_per_file" => statements_per_file, "lines_per_file" => lines_per_file, "functions_per_file" => functions_per_file,
        "arguments_per_function" => arguments_per_function,
        "arguments_positional" => arguments_positional, "arguments_keyword_only" => arguments_keyword_only,
        "max_indentation_depth" => max_indentation_depth,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "classes_per_file" => concrete_types_per_file,
        "nested_function_depth" => nested_function_depth, "returns_per_function" => returns_per_function,
        "return_values_per_function" => return_values_per_function,
        "branches_per_function" => branches_per_function, "local_variables_per_function" => local_variables_per_function,
        "imported_names_per_file" => imported_names_per_file,
        "statements_per_try_block" => statements_per_try_block,
        "boolean_parameters" => boolean_parameters,
        "annotations_per_function" => annotations_per_function,
        "calls_per_function" => calls_per_function,
        "cycle_size" => cycle_size,
        "indirect_dependencies" => indirect_dependencies,
        "dependency_depth" => dependency_depth);
}

pub(crate) fn apply_shared(config: &mut Config, table: &toml::Table) {
    if let Err(e) = check_unknown_keys(table, SHARED_KEYS, "shared") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_file" => statements_per_file,
        "lines_per_file" => lines_per_file,
        "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "imported_names_per_file" => imported_names_per_file,
        "cycle_size" => cycle_size,
        "indirect_dependencies" => indirect_dependencies,
        "dependency_depth" => dependency_depth);
}

pub(crate) fn apply_python(config: &mut Config, table: &toml::Table) {
    if let Err(e) = check_unknown_keys(table, PYTHON_KEYS, "python") {
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
        "statements_per_file" => statements_per_file,
        "lines_per_file" => lines_per_file,
        "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "cycle_size" => cycle_size, "indirect_dependencies" => indirect_dependencies, "dependency_depth" => dependency_depth);
}

pub(crate) fn apply_rust(config: &mut Config, table: &toml::Table) {
    if let Err(e) = check_unknown_keys(table, RUST_KEYS, "rust") {
        eprintln!("Error: {e}");
        return;
    }
    apply_config!(config, table,
        "statements_per_function" => statements_per_function, "arguments" => arguments_per_function,
        "max_indentation" => max_indentation_depth, "branches_per_function" => branches_per_function,
        "local_variables" => local_variables_per_function, "methods_per_class" => methods_per_class,
        "statements_per_file" => statements_per_file,
        "lines_per_file" => lines_per_file,
        "functions_per_file" => functions_per_file,
        "interface_types_per_file" => interface_types_per_file,
        "concrete_types_per_file" => concrete_types_per_file,
        // Back-compat alias.
        "types_per_file" => concrete_types_per_file,
        "returns_per_function" => returns_per_function,
        "nested_function_depth" => nested_function_depth, "boolean_parameters" => boolean_parameters,
        "attributes_per_function" => annotations_per_function, "calls_per_function" => calls_per_function,
        "imported_names_per_file" => imported_names_per_file,
        "cycle_size" => cycle_size, "indirect_dependencies" => indirect_dependencies, "dependency_depth" => dependency_depth);
}

pub(super) fn apply_thresholds_and_shared(config: &mut Config, table: &toml::Table) {
    if let Some(t) = table.get("thresholds").and_then(|v| v.as_table()) {
        apply_thresholds(config, t);
    }
    if let Some(t) = table.get("shared").and_then(|v| v.as_table()) {
        apply_shared(config, t);
    }
}

pub(super) fn apply_python_if_present(config: &mut Config, table: &toml::Table) {
    if let Some(t) = table.get("python").and_then(|v| v.as_table()) {
        apply_python(config, t);
    }
}

pub(super) fn apply_rust_if_present(config: &mut Config, table: &toml::Table) {
    if let Some(t) = table.get("rust").and_then(|v| v.as_table()) {
        apply_rust(config, t);
    }
}

pub(super) fn apply_language_sections(config: &mut Config, table: &toml::Table, lang: Option<ConfigLanguage>) {
    match lang {
        Some(ConfigLanguage::Python) => apply_python_if_present(config, table),
        Some(ConfigLanguage::Rust) => apply_rust_if_present(config, table),
        None => {
            apply_python_if_present(config, table);
            apply_rust_if_present(config, table);
        }
    }
}

pub(super) fn apply_parsed_toml(config: &mut Config, table: &toml::Table, lang: Option<ConfigLanguage>) {
    apply_thresholds_and_shared(config, table);
    apply_language_sections(config, table, lang);
}

impl Config {
    pub(crate) fn merge_from_toml(&mut self, content: &str, lang: Option<ConfigLanguage>) {
        self.merge_from_toml_with_path(content, lang, None);
    }

    /// Result-based merge that returns errors instead of printing to stderr.
    pub(crate) fn try_merge_from_toml(
        &mut self,
        content: &str,
        lang: Option<ConfigLanguage>,
    ) -> Result<(), ConfigError> {
        let table = content
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        check_unknown_sections(&table)?;
        validate_config_keys(&table, lang)?;
        // All validations passed, apply using the regular merge (which won't print errors)
        self.merge_from_toml_with_path(content, lang, None);
        Ok(())
    }

    pub(crate) fn merge_from_toml_with_path(
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
        apply_parsed_toml(self, &table, lang);
    }
}
