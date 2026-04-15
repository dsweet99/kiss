use super::definitions::METRICS;
use super::metric_stats::MetricStats;
use super::percentile::PercentileSummary;

pub fn compute_summaries(stats: &MetricStats) -> Vec<PercentileSummary> {
    METRICS
        .iter()
        .filter_map(|m| {
            let values = metric_values(stats, m.metric_id)?;
            if values.is_empty() {
                None
            } else {
                Some(PercentileSummary::from_values(m.metric_id, values))
            }
        })
        .collect()
}

pub(crate) fn metric_values<'a>(stats: &'a MetricStats, metric_id: &str) -> Option<&'a [usize]> {
    Some(match metric_id {
        "statements_per_function" => &stats.statements_per_function,
        "arguments_per_function" => &stats.arguments_per_function,
        "positional_args" => &stats.arguments_positional,
        "keyword_only_args" => &stats.arguments_keyword_only,
        "max_indentation_depth" => &stats.max_indentation,
        "nested_function_depth" => &stats.nested_function_depth,
        "returns_per_function" => &stats.returns_per_function,
        "return_values_per_function" => &stats.return_values_per_function,
        "branches_per_function" => &stats.branches_per_function,
        "local_variables_per_function" => &stats.local_variables_per_function,
        "statements_per_try_block" => &stats.statements_per_try_block,
        "boolean_parameters" => &stats.boolean_parameters,
        "annotations_per_function" => &stats.annotations_per_function,
        "calls_per_function" => &stats.calls_per_function,
        "methods_per_class" => &stats.methods_per_class,
        "statements_per_file" => &stats.statements_per_file,
        "lines_per_file" => &stats.lines_per_file,
        "functions_per_file" => &stats.functions_per_file,
        "interface_types_per_file" => &stats.interface_types_per_file,
        "concrete_types_per_file" => &stats.concrete_types_per_file,
        "imported_names_per_file" => &stats.imported_names_per_file,
        "fan_in" => &stats.fan_in,
        "fan_out" => &stats.fan_out,
        "cycle_size" => &stats.cycle_size,
        "indirect_dependencies" => &stats.indirect_dependencies,
        "dependency_depth" => &stats.dependency_depth,
        _ => return None,
    })
}
