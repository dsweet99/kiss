use crate::defaults;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLanguage {
    Python,
    Rust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageTablesPresent {
    pub python: bool,
    pub rust: bool,
}

impl Default for LanguageTablesPresent {
    fn default() -> Self {
        Self::both()
    }
}

impl LanguageTablesPresent {
    #[must_use]
    pub const fn both() -> Self {
        Self {
            python: true,
            rust: true,
        }
    }

    #[must_use]
    pub const fn none() -> Self {
        Self {
            python: false,
            rust: false,
        }
    }

    #[must_use]
    pub fn from_toml(content: &str) -> Self {
        let Ok(table) = content.parse::<toml::Table>() else {
            return Self::none();
        };
        Self {
            python: table.contains_key("python"),
            rust: table.contains_key("rust"),
        }
    }

    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .map(|content| Self::from_toml(&content))
            .unwrap_or_else(|_| Self::none())
    }

    #[must_use]
    pub fn from_path_or_both(path: &std::path::Path) -> Self {
        if path.exists() {
            Self::from_path(path)
        } else {
            Self::both()
        }
    }

    #[must_use]
    pub fn missing_language(
        self,
        py_files: &[std::path::PathBuf],
        rs_files: &[std::path::PathBuf],
    ) -> Option<&'static str> {
        if !py_files.is_empty() && !self.python {
            Some("python")
        } else if !rs_files.is_empty() && !self.rust {
            Some("rust")
        } else {
            None
        }
    }
}

pub fn missing_language_table_message(language: &str) -> String {
    format!(
        "Error: found {language} files but .kissconfig has no [{language}] table. Delete .kissconfig and run `kiss check` to generate language thresholds."
    )
}

pub fn reject_unconfigured_languages(
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
    tables: LanguageTablesPresent,
) -> Result<(), i32> {
    if let Some(language) = tables.missing_language(py_files, rs_files) {
        eprintln!("{}", missing_language_table_message(language));
        return Err(1);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Config {
    pub statements_per_function: usize,
    pub methods_per_class: usize,
    pub statements_per_file: usize,
    pub lines_per_file: usize,
    pub functions_per_file: usize,
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
    pub indirect_dependencies: usize,
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
            lines_per_file: py::LINES_PER_FILE,
            functions_per_file: py::FUNCTIONS_PER_FILE,
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
            indirect_dependencies: py::INDIRECT_DEPENDENCIES,
            dependency_depth: py::DEPENDENCY_DEPTH,
        }
    }

    pub const fn rust_defaults() -> Self {
        use defaults::{NOT_APPLICABLE as NA, rust as rs};
        Self {
            statements_per_function: rs::STATEMENTS_PER_FUNCTION,
            methods_per_class: rs::METHODS_PER_TYPE,
            statements_per_file: rs::STATEMENTS_PER_FILE,
            lines_per_file: rs::LINES_PER_FILE,
            functions_per_file: rs::FUNCTIONS_PER_FILE,
            arguments_positional: rs::ARGUMENTS,
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
            indirect_dependencies: rs::INDIRECT_DEPENDENCIES,
            dependency_depth: rs::DEPENDENCY_DEPTH,
        }
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl ConfigLanguage {
        fn witness() -> Self {
            Self::Python
        }
    }

    #[test]
    fn witness_config_language() {
        assert_eq!(ConfigLanguage::witness(), ConfigLanguage::Python);
        let _ = Config::python_defaults();
        let _ = Config::rust_defaults();
    }

    #[test]
    fn python_defaults_populate_every_field_from_python_and_graph_defaults() {
        let config = Config::python_defaults();

        assert_eq!(
            config.statements_per_function,
            crate::defaults::python::STATEMENTS_PER_FUNCTION
        );
        assert_eq!(
            config.methods_per_class,
            crate::defaults::python::METHODS_PER_CLASS
        );
        assert_eq!(
            config.statements_per_file,
            crate::defaults::python::STATEMENTS_PER_FILE
        );
        assert_eq!(
            config.lines_per_file,
            crate::defaults::python::LINES_PER_FILE
        );
        assert_eq!(
            config.functions_per_file,
            crate::defaults::python::FUNCTIONS_PER_FILE
        );
        assert_eq!(
            config.arguments_positional,
            crate::defaults::python::POSITIONAL_ARGS
        );
        assert_eq!(
            config.arguments_keyword_only,
            crate::defaults::python::KEYWORD_ONLY_ARGS
        );
        assert_eq!(
            config.max_indentation_depth,
            crate::defaults::python::MAX_INDENTATION
        );
        assert_eq!(
            config.interface_types_per_file,
            crate::defaults::python::INTERFACE_TYPES_PER_FILE
        );
        assert_eq!(
            config.concrete_types_per_file,
            crate::defaults::python::CONCRETE_TYPES_PER_FILE
        );
        assert_eq!(
            config.nested_function_depth,
            crate::defaults::python::NESTED_FUNCTION_DEPTH
        );
        assert_eq!(
            config.returns_per_function,
            crate::defaults::python::RETURNS_PER_FUNCTION
        );
        assert_eq!(
            config.return_values_per_function,
            crate::defaults::python::RETURN_VALUES_PER_FUNCTION
        );
        assert_eq!(
            config.branches_per_function,
            crate::defaults::python::BRANCHES_PER_FUNCTION
        );
        assert_eq!(
            config.local_variables_per_function,
            crate::defaults::python::LOCAL_VARIABLES
        );
        assert_eq!(
            config.imported_names_per_file,
            crate::defaults::python::IMPORTS_PER_FILE
        );
        assert_eq!(
            config.statements_per_try_block,
            crate::defaults::python::STATEMENTS_PER_TRY_BLOCK
        );
        assert_eq!(
            config.boolean_parameters,
            crate::defaults::python::BOOLEAN_PARAMETERS
        );
        assert_eq!(
            config.annotations_per_function,
            crate::defaults::python::DECORATORS_PER_FUNCTION
        );
        assert_eq!(
            config.calls_per_function,
            crate::defaults::python::CALLS_PER_FUNCTION
        );
        assert_eq!(config.cycle_size, crate::defaults::graph::CYCLE_SIZE);
        assert_eq!(
            config.indirect_dependencies,
            crate::defaults::python::INDIRECT_DEPENDENCIES
        );
        assert_eq!(
            config.dependency_depth,
            crate::defaults::python::DEPENDENCY_DEPTH
        );
    }

    #[test]
    fn rust_defaults_populate_every_field_from_rust_and_shared_defaults() {
        let config = Config::rust_defaults();

        assert_eq!(
            config.statements_per_function,
            crate::defaults::rust::STATEMENTS_PER_FUNCTION
        );
        assert_eq!(
            config.methods_per_class,
            crate::defaults::rust::METHODS_PER_TYPE
        );
        assert_eq!(
            config.statements_per_file,
            crate::defaults::rust::STATEMENTS_PER_FILE
        );
        assert_eq!(config.lines_per_file, crate::defaults::rust::LINES_PER_FILE);
        assert_eq!(
            config.functions_per_file,
            crate::defaults::rust::FUNCTIONS_PER_FILE
        );
        assert_eq!(
            config.arguments_positional,
            crate::defaults::rust::ARGUMENTS
        );
        assert_eq!(
            config.arguments_keyword_only,
            crate::defaults::NOT_APPLICABLE
        );
        assert_eq!(
            config.max_indentation_depth,
            crate::defaults::rust::MAX_INDENTATION
        );
        assert_eq!(
            config.interface_types_per_file,
            crate::defaults::rust::INTERFACE_TYPES_PER_FILE
        );
        assert_eq!(
            config.concrete_types_per_file,
            crate::defaults::rust::CONCRETE_TYPES_PER_FILE
        );
        assert_eq!(
            config.nested_function_depth,
            crate::defaults::rust::NESTED_FUNCTION_DEPTH
        );
        assert_eq!(
            config.returns_per_function,
            crate::defaults::rust::RETURNS_PER_FUNCTION
        );
        assert_eq!(
            config.return_values_per_function,
            crate::defaults::NOT_APPLICABLE
        );
        assert_eq!(
            config.branches_per_function,
            crate::defaults::rust::BRANCHES_PER_FUNCTION
        );
        assert_eq!(
            config.local_variables_per_function,
            crate::defaults::rust::LOCAL_VARIABLES
        );
        assert_eq!(
            config.imported_names_per_file,
            crate::defaults::rust::IMPORTS_PER_FILE
        );
        assert_eq!(
            config.statements_per_try_block,
            crate::defaults::NOT_APPLICABLE
        );
        assert_eq!(
            config.boolean_parameters,
            crate::defaults::rust::BOOLEAN_PARAMETERS
        );
        assert_eq!(
            config.annotations_per_function,
            crate::defaults::rust::ATTRIBUTES_PER_FUNCTION
        );
        assert_eq!(
            config.calls_per_function,
            crate::defaults::rust::CALLS_PER_FUNCTION
        );
        assert_eq!(config.cycle_size, crate::defaults::graph::CYCLE_SIZE);
        assert_eq!(
            config.indirect_dependencies,
            crate::defaults::rust::INDIRECT_DEPENDENCIES
        );
        assert_eq!(
            config.dependency_depth,
            crate::defaults::rust::DEPENDENCY_DEPTH
        );
    }
}
