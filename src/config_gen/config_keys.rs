/// Map `metric_id` to config key (common keys shared by Python and Rust)
pub(crate) fn common_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "statements_per_function" => Some("statements_per_function"),
        "max_indentation_depth" => Some("max_indentation"),
        "branches_per_function" => Some("branches_per_function"),
        "local_variables_per_function" => Some("local_variables"),
        "cycle_size" => Some("cycle_size"),
        "methods_per_class" => Some("methods_per_class"),
        "nested_function_depth" => Some("nested_function_depth"),
        "returns_per_function" => Some("returns_per_function"),
        "calls_per_function" => Some("calls_per_function"),
        "statements_per_file" => Some("statements_per_file"),
        "lines_per_file" => Some("lines_per_file"),
        "functions_per_file" => Some("functions_per_file"),
        "interface_types_per_file" => Some("interface_types_per_file"),
        "concrete_types_per_file" => Some("concrete_types_per_file"),
        "imported_names_per_file" => Some("imported_names_per_file"),
        "indirect_dependencies" => Some("indirect_dependencies"),
        "dependency_depth" => Some("dependency_depth"),
        _ => None,
    }
}

pub fn python_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "positional_args" => Some("positional_args"),
        "keyword_only_args" => Some("keyword_only_args"),
        "return_values_per_function" => Some("return_values_per_function"),
        "statements_per_try_block" => Some("statements_per_try_block"),
        "boolean_parameters" => Some("boolean_parameters"),
        "annotations_per_function" => Some("decorators_per_function"),
        _ => common_config_key(metric_id),
    }
}

pub fn rust_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "arguments_per_function" => Some("arguments"),
        "boolean_parameters" => Some("boolean_parameters"),
        "annotations_per_function" => Some("attributes_per_function"),
        _ => common_config_key(metric_id),
    }
}
