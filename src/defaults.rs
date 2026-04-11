/// Threshold value that effectively disables a check (for N/A metrics in a language)
pub const NOT_APPLICABLE: usize = usize::MAX;

pub mod python {
    pub const IMPORTS_PER_FILE: usize = 30;
    pub const STATEMENTS_PER_FILE: usize = 200;
    pub const LINES_PER_FILE: usize = 300;
    pub const FUNCTIONS_PER_FILE: usize = 10;
    pub const INTERFACE_TYPES_PER_FILE: usize = 1;
    pub const CONCRETE_TYPES_PER_FILE: usize = 1;
    pub const STATEMENTS_PER_FUNCTION: usize = 35;
    pub const ARGUMENTS_PER_FUNCTION: usize = 7;
    pub const POSITIONAL_ARGS: usize = 3;
    pub const KEYWORD_ONLY_ARGS: usize = 3;
    pub const MAX_INDENTATION: usize = 4;
    pub const BRANCHES_PER_FUNCTION: usize = 9;
    pub const LOCAL_VARIABLES: usize = 15;
    pub const METHODS_PER_CLASS: usize = 10;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const RETURN_VALUES_PER_FUNCTION: usize = 3;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const STATEMENTS_PER_TRY_BLOCK: usize = 3;
    pub const BOOLEAN_PARAMETERS: usize = 1;
    pub const DECORATORS_PER_FUNCTION: usize = 5;
    pub const CALLS_PER_FUNCTION: usize = 20;
    pub const INDIRECT_DEPENDENCIES: usize = 10;
    pub const DEPENDENCY_DEPTH: usize = 3;
}

pub mod rust {
    pub const IMPORTS_PER_FILE: usize = 50;
    pub const STATEMENTS_PER_FILE: usize = 250;
    pub const LINES_PER_FILE: usize = 900;
    pub const FUNCTIONS_PER_FILE: usize = 40;
    pub const INTERFACE_TYPES_PER_FILE: usize = 0;
    pub const CONCRETE_TYPES_PER_FILE: usize = 8;
    pub const STATEMENTS_PER_FUNCTION: usize = 35;
    pub const ARGUMENTS: usize = 8;
    pub const MAX_INDENTATION: usize = 5;
    pub const BRANCHES_PER_FUNCTION: usize = 9;
    pub const LOCAL_VARIABLES: usize = 20;
    pub const METHODS_PER_TYPE: usize = 10;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const BOOLEAN_PARAMETERS: usize = 1;
    pub const ATTRIBUTES_PER_FUNCTION: usize = 1;
    pub const CALLS_PER_FUNCTION: usize = 45;
    pub const INDIRECT_DEPENDENCIES: usize = 10;
    pub const DEPENDENCY_DEPTH: usize = 3;
}

pub mod graph {
    pub const CYCLE_SIZE: usize = 0;
}

pub mod duplication {
    pub const MIN_SIMILARITY: f64 = 0.7;
}

pub mod gate {
    pub const TEST_COVERAGE_THRESHOLD: usize = 90;
}

pub fn default_config_toml() -> String {
    format!(
        r"# Default kiss configuration

[gate]
test_coverage_threshold = {gate_coverage}
min_similarity = {min_sim}
duplication_enabled = true
orphan_module_enabled = true

[python]
statements_per_function = {py_statements}
positional_args = {py_pos_args}
keyword_only_args = {py_kw_args}
max_indentation = {py_indent}
nested_function_depth = {py_nested}
returns_per_function = {py_returns}
return_values_per_function = {py_return_values}
branches_per_function = {py_branches}
local_variables = {py_locals}
statements_per_try_block = {py_try_stmts}
boolean_parameters = {py_bool_params}
decorators_per_function = {py_decorators}
calls_per_function = {py_calls}
statements_per_file = {py_statements_file}
lines_per_file = {py_lines_file}
functions_per_file = {py_functions_file}
interface_types_per_file = {py_interface_types}
concrete_types_per_file = {py_concrete_types}
imported_names_per_file = {py_imports}
cycle_size = {cycle_size}
indirect_dependencies = {py_indirect_deps}
dependency_depth = {py_dep_depth}

[rust]
statements_per_function = {rs_statements}
arguments = {rs_args}
max_indentation = {rs_indent}
nested_function_depth = {rs_nested}
returns_per_function = {rs_returns}
branches_per_function = {rs_branches}
local_variables = {rs_locals}
boolean_parameters = {rs_bool_params}
attributes_per_function = {rs_attrs}
calls_per_function = {rs_calls}
methods_per_class = {rs_methods}
statements_per_file = {rs_statements_file}
lines_per_file = {rs_lines_file}
functions_per_file = {rs_functions_file}
interface_types_per_file = {rs_interface_types}
concrete_types_per_file = {rs_concrete_types}
imported_names_per_file = {rs_imports}
cycle_size = {cycle_size}
indirect_dependencies = {rs_indirect_deps}
dependency_depth = {rs_dep_depth}
",
        gate_coverage = gate::TEST_COVERAGE_THRESHOLD,
        min_sim = duplication::MIN_SIMILARITY,
        py_statements = python::STATEMENTS_PER_FUNCTION,
        py_pos_args = python::POSITIONAL_ARGS,
        py_kw_args = python::KEYWORD_ONLY_ARGS,
        py_indent = python::MAX_INDENTATION,
        py_nested = python::NESTED_FUNCTION_DEPTH,
        py_returns = python::RETURNS_PER_FUNCTION,
        py_return_values = python::RETURN_VALUES_PER_FUNCTION,
        py_branches = python::BRANCHES_PER_FUNCTION,
        py_locals = python::LOCAL_VARIABLES,
        py_try_stmts = python::STATEMENTS_PER_TRY_BLOCK,
        py_bool_params = python::BOOLEAN_PARAMETERS,
        py_decorators = python::DECORATORS_PER_FUNCTION,
        py_calls = python::CALLS_PER_FUNCTION,
        py_statements_file = python::STATEMENTS_PER_FILE,
        py_lines_file = python::LINES_PER_FILE,
        py_functions_file = python::FUNCTIONS_PER_FILE,
        py_interface_types = python::INTERFACE_TYPES_PER_FILE,
        py_concrete_types = python::CONCRETE_TYPES_PER_FILE,
        py_imports = python::IMPORTS_PER_FILE,
        cycle_size = graph::CYCLE_SIZE,
        py_indirect_deps = python::INDIRECT_DEPENDENCIES,
        py_dep_depth = python::DEPENDENCY_DEPTH,
        rs_statements = rust::STATEMENTS_PER_FUNCTION,
        rs_args = rust::ARGUMENTS,
        rs_indent = rust::MAX_INDENTATION,
        rs_nested = rust::NESTED_FUNCTION_DEPTH,
        rs_returns = rust::RETURNS_PER_FUNCTION,
        rs_branches = rust::BRANCHES_PER_FUNCTION,
        rs_locals = rust::LOCAL_VARIABLES,
        rs_bool_params = rust::BOOLEAN_PARAMETERS,
        rs_attrs = rust::ATTRIBUTES_PER_FUNCTION,
        rs_calls = rust::CALLS_PER_FUNCTION,
        rs_methods = rust::METHODS_PER_TYPE,
        rs_statements_file = rust::STATEMENTS_PER_FILE,
        rs_lines_file = rust::LINES_PER_FILE,
        rs_functions_file = rust::FUNCTIONS_PER_FILE,
        rs_interface_types = rust::INTERFACE_TYPES_PER_FILE,
        rs_concrete_types = rust::CONCRETE_TYPES_PER_FILE,
        rs_imports = rust::IMPORTS_PER_FILE,
        rs_indirect_deps = rust::INDIRECT_DEPENDENCIES,
        rs_dep_depth = rust::DEPENDENCY_DEPTH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_defaults_are_reasonable() {
        assert!(python::STATEMENTS_PER_FUNCTION > 0);
        assert!(rust::STATEMENTS_PER_FUNCTION > 0);
        assert!(gate::TEST_COVERAGE_THRESHOLD <= 100);
    }

    #[test]
    fn test_default_config_toml_parses() {
        let toml = default_config_toml();
        assert!(toml.parse::<toml::Table>().is_ok());
    }
}
