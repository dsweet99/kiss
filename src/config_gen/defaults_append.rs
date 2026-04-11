use std::fmt::Write;

pub fn append_python_defaults(out: &mut String) {
    use crate::defaults::{graph, python};
    let _ = writeln!(out, "[python]");
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        python::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "positional_args = {}", python::POSITIONAL_ARGS);
    let _ = writeln!(out, "keyword_only_args = {}", python::KEYWORD_ONLY_ARGS);
    let _ = writeln!(out, "max_indentation = {}", python::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        python::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", python::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", python::METHODS_PER_CLASS);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        python::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(
        out,
        "returns_per_function = {}",
        python::RETURNS_PER_FUNCTION
    );
    let _ = writeln!(out, "statements_per_file = {}", python::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "lines_per_file = {}", python::LINES_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", python::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        python::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        python::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "imported_names_per_file = {}",
        python::IMPORTS_PER_FILE
    );
    let _ = writeln!(
        out,
        "indirect_dependencies = {}",
        python::INDIRECT_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", python::DEPENDENCY_DEPTH);
    let _ = writeln!(
        out,
        "statements_per_try_block = {}",
        python::STATEMENTS_PER_TRY_BLOCK
    );
    let _ = writeln!(out, "boolean_parameters = {}", python::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "decorators_per_function = {}",
        python::DECORATORS_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", python::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}

pub fn append_rust_defaults(out: &mut String) {
    use crate::defaults::{graph, rust};
    let _ = writeln!(out, "[rust]");
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        rust::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "arguments = {}", rust::ARGUMENTS);
    let _ = writeln!(out, "max_indentation = {}", rust::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        rust::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", rust::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", rust::METHODS_PER_TYPE);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        rust::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(out, "returns_per_function = {}", rust::RETURNS_PER_FUNCTION);
    let _ = writeln!(out, "statements_per_file = {}", rust::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "lines_per_file = {}", rust::LINES_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", rust::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        rust::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        rust::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(out, "imported_names_per_file = {}", rust::IMPORTS_PER_FILE);
    let _ = writeln!(
        out,
        "indirect_dependencies = {}",
        rust::INDIRECT_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", rust::DEPENDENCY_DEPTH);
    let _ = writeln!(out, "boolean_parameters = {}", rust::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "attributes_per_function = {}",
        rust::ATTRIBUTES_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", rust::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}
