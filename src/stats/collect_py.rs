use super::metric_stats::MetricStats;
use crate::py_metrics::PyWalkAction;

pub(crate) struct StatsVisitor<'a> {
    pub(crate) stats: &'a mut MetricStats,
}

impl StatsVisitor<'_> {
    pub(crate) fn process(&mut self, action: PyWalkAction<'_>) {
        match action {
            PyWalkAction::Function(visit) => push_py_fn_metrics(self.stats, visit.metrics),
            PyWalkAction::Class(visit) => self.stats.methods_per_class.push(visit.metrics.methods),
        }
    }
}

pub(crate) fn push_py_fn_metrics(stats: &mut MetricStats, m: &crate::py_metrics::FunctionMetrics) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_per_function.push(m.arguments);
    stats.arguments_positional.push(m.arguments_positional);
    stats.arguments_keyword_only.push(m.arguments_keyword_only);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    stats.return_values_per_function.push(m.max_return_values);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    stats
        .statements_per_try_block
        .push(m.max_try_block_statements);
    stats.boolean_parameters.push(m.boolean_parameters);
    stats.annotations_per_function.push(m.decorators);
    stats.calls_per_function.push(m.calls);
}
