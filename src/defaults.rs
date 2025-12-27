pub mod python {
    pub const IMPORTS_PER_FILE: usize = 20;
    pub const STATEMENTS_PER_FILE: usize = 400;
    pub const TYPES_PER_FILE: usize = 10;
    pub const STATEMENTS_PER_FUNCTION: usize = 35;
    pub const ARGUMENTS_PER_FUNCTION: usize = 7;
    pub const POSITIONAL_ARGS: usize = 5;
    pub const KEYWORD_ONLY_ARGS: usize = 6;
    pub const MAX_INDENTATION: usize = 4;
    pub const BRANCHES_PER_FUNCTION: usize = 10;
    pub const LOCAL_VARIABLES: usize = 20;
    pub const METHODS_PER_CLASS: usize = 20;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const STATEMENTS_PER_TRY_BLOCK: usize = 5;
    pub const BOOLEAN_PARAMETERS: usize = 1;
    pub const DECORATORS_PER_FUNCTION: usize = 3;
}

pub mod rust {
    pub const IMPORTS_PER_FILE: usize = 20;
    pub const STATEMENTS_PER_FILE: usize = 300;
    pub const TYPES_PER_FILE: usize = 8;
    pub const STATEMENTS_PER_FUNCTION: usize = 25;
    pub const ARGUMENTS: usize = 8;
    pub const MAX_INDENTATION: usize = 4;
    pub const BRANCHES_PER_FUNCTION: usize = 8;
    pub const LOCAL_VARIABLES: usize = 20;
    pub const METHODS_PER_TYPE: usize = 15;
    pub const RETURNS_PER_FUNCTION: usize = 5;
    pub const NESTED_FUNCTION_DEPTH: usize = 2;
    pub const BOOLEAN_PARAMETERS: usize = 2;
    pub const ATTRIBUTES_PER_FUNCTION: usize = 4;
}

pub mod graph {
    pub const CYCLE_SIZE: usize = 3;
    pub const TRANSITIVE_DEPENDENCIES: usize = 30;
    pub const DEPENDENCY_DEPTH: usize = 4;
}

pub mod duplication {
    pub const MIN_SIMILARITY: f64 = 0.7;
}

pub mod gate {
    pub const TEST_COVERAGE_THRESHOLD: usize = 90;
}

pub fn default_config_toml() -> String {
    format!(r"[gate]
test_coverage_threshold = {gate_coverage}
min_similarity = {min_sim}

[python]
imported_names_per_file = {py_imports}
statements_per_file = {py_statements_file}
types_per_file = {py_types}
statements_per_function = {py_statements}
positional_args = {py_pos_args}
keyword_only_args = {py_kw_args}
max_indentation = {py_indent}
branches_per_function = {py_branches}
local_variables = {py_locals}
methods_per_class = {py_methods}
returns_per_function = {py_returns}
nested_function_depth = {py_nested}
statements_per_try_block = {py_try_stmts}
boolean_parameters = {py_bool_params}
decorators_per_function = {py_decorators}

cycle_size = {cycle_size}
transitive_dependencies = {transitive_deps}
dependency_depth = {dep_depth}

[rust]
imported_names_per_file = {rs_imports}
statements_per_file = {rs_statements_file}
types_per_file = {rs_types}
statements_per_function = {rs_statements}
arguments = {rs_args}
max_indentation = {rs_indent}
branches_per_function = {rs_branches}
local_variables = {rs_locals}
methods_per_class = {rs_methods}
returns_per_function = {rs_returns}
nested_function_depth = {rs_nested}
boolean_parameters = {rs_bool_params}
attributes_per_function = {rs_attrs}
",
        gate_coverage = gate::TEST_COVERAGE_THRESHOLD,
        min_sim = duplication::MIN_SIMILARITY,
        py_imports = python::IMPORTS_PER_FILE,
        py_statements_file = python::STATEMENTS_PER_FILE,
        py_types = python::TYPES_PER_FILE,
        py_statements = python::STATEMENTS_PER_FUNCTION,
        py_pos_args = python::POSITIONAL_ARGS,
        py_kw_args = python::KEYWORD_ONLY_ARGS,
        py_indent = python::MAX_INDENTATION,
        py_branches = python::BRANCHES_PER_FUNCTION,
        py_locals = python::LOCAL_VARIABLES,
        py_methods = python::METHODS_PER_CLASS,
        py_returns = python::RETURNS_PER_FUNCTION,
        py_nested = python::NESTED_FUNCTION_DEPTH,
        py_try_stmts = python::STATEMENTS_PER_TRY_BLOCK,
        py_bool_params = python::BOOLEAN_PARAMETERS,
        py_decorators = python::DECORATORS_PER_FUNCTION,
        cycle_size = graph::CYCLE_SIZE,
        transitive_deps = graph::TRANSITIVE_DEPENDENCIES,
        dep_depth = graph::DEPENDENCY_DEPTH,
        rs_imports = rust::IMPORTS_PER_FILE,
        rs_statements_file = rust::STATEMENTS_PER_FILE,
        rs_types = rust::TYPES_PER_FILE,
        rs_statements = rust::STATEMENTS_PER_FUNCTION,
        rs_args = rust::ARGUMENTS,
        rs_indent = rust::MAX_INDENTATION,
        rs_branches = rust::BRANCHES_PER_FUNCTION,
        rs_locals = rust::LOCAL_VARIABLES,
        rs_methods = rust::METHODS_PER_TYPE,
        rs_returns = rust::RETURNS_PER_FUNCTION,
        rs_nested = rust::NESTED_FUNCTION_DEPTH,
        rs_bool_params = rust::BOOLEAN_PARAMETERS,
        rs_attrs = rust::ATTRIBUTES_PER_FUNCTION,
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
