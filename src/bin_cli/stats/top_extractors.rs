type UnitMetricExtractor = fn(&kiss::UnitMetrics) -> Option<usize>;

pub(crate) fn extractor_for_fn_core(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        "statements_per_function" => Some(|u| u.statements),
        "positional_args" => Some(|u| u.args_positional),
        "keyword_only_args" => Some(|u| u.args_keyword_only),
        "max_indentation_depth" => Some(|u| u.indentation),
        "nested_function_depth" => Some(|u| u.nested_depth),
        "returns_per_function" => Some(|u| u.returns),
        "return_values_per_function" => Some(|u| u.return_values),
        _ => None,
    }
}

pub(crate) fn extractor_for_fn_extra(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        "branches_per_function" => Some(|u| u.branches),
        "local_variables_per_function" => Some(|u| u.locals),
        "statements_per_try_block" => Some(|u| u.try_block_statements),
        "boolean_parameters" => Some(|u| u.boolean_parameters),
        "annotations_per_function" => Some(|u| u.annotations),
        "calls_per_function" => Some(|u| u.calls),
        "methods_per_class" => Some(|u| u.methods),
        _ => None,
    }
}

pub(crate) fn extractor_for_fn(metric_id: &str) -> Option<UnitMetricExtractor> {
    extractor_for_fn_core(metric_id).or_else(|| extractor_for_fn_extra(metric_id))
}

pub(crate) fn extractor_for_file(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        "statements_per_file" => Some(|u| u.file_statements),
        "lines_per_file" => Some(|u| u.lines),
        "functions_per_file" => Some(|u| u.file_functions),
        "interface_types_per_file" => Some(|u| u.interface_types),
        "concrete_types_per_file" => Some(|u| u.concrete_types),
        "imported_names_per_file" => Some(|u| u.imports),
        "inv_test_coverage" => Some(|u| u.inv_test_coverage),
        _ => None,
    }
}

pub(crate) fn extractor_for_graph(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        "fan_in" => Some(|u| u.fan_in),
        "fan_out" => Some(|u| u.fan_out),
        "indirect_dependencies" => Some(|u| u.indirect_deps),
        "dependency_depth" => Some(|u| u.dependency_depth),
        "cycle_size" => Some(|u| u.cycle_size),
        _ => None,
    }
}

pub(crate) fn extractor_for(metric_id: &str) -> Option<UnitMetricExtractor> {
    extractor_for_fn(metric_id)
        .or_else(|| extractor_for_file(metric_id))
        .or_else(|| extractor_for_graph(metric_id))
}
