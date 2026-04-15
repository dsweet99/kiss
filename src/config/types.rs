use crate::defaults;

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
    pub lines_per_file: usize,
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
            indirect_dependencies: rs::INDIRECT_DEPENDENCIES,
            dependency_depth: rs::DEPENDENCY_DEPTH,
        }
    }
}
