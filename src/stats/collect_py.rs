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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{ParsedFile, create_parser};
    use crate::py_metrics::walk_py_ast;
    use crate::py_metrics::{ClassMetrics, ClassVisit, FunctionMetrics, FunctionVisit};
    use std::path::PathBuf;

    #[test]
    fn stats_visitor_process_handles_direct_actions() {
        let fn_metrics = FunctionMetrics {
            statements: 1,
            arguments_positional: 2,
            returns: 3,
            ..Default::default()
        };
        let class_metrics = ClassMetrics { methods: 4 };
        let mut stats = MetricStats::default();
        let mut visitor = StatsVisitor { stats: &mut stats };

        visitor.process(crate::py_metrics::PyWalkAction::Function(FunctionVisit {
            metrics: &fn_metrics,
            name: "f",
            line: 10,
            inside_class: false,
        }));
        visitor.process(crate::py_metrics::PyWalkAction::Class(ClassVisit {
            metrics: &class_metrics,
            name: "C",
            line: 20,
        }));

        assert_eq!(visitor.stats.statements_per_function, vec![1]);
        assert_eq!(visitor.stats.arguments_positional, vec![2]);
        assert_eq!(visitor.stats.returns_per_function, vec![3]);
        assert_eq!(visitor.stats.methods_per_class, vec![4]);
    }

    #[test]
    fn stats_visitor_process_collects_function_and_class_metrics() {
        let source = "class C:\n    def m(self, flag=False):\n        return flag\n";
        let mut parser = create_parser().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let parsed = ParsedFile {
            path: PathBuf::from("sample.py"),
            source: source.to_string(),
            tree,
        };
        let mut stats = MetricStats::default();
        let mut visitor = StatsVisitor { stats: &mut stats };
        walk_py_ast(
            parsed.tree.root_node(),
            &parsed.source,
            &mut |action| visitor.process(action),
            false,
        );

        assert_eq!(visitor.stats.methods_per_class, vec![1]);
        assert_eq!(visitor.stats.arguments_positional, vec![1]);
        assert_eq!(visitor.stats.boolean_parameters, vec![1]);
        assert_eq!(visitor.stats.returns_per_function, vec![1]);
    }

    #[test]
    fn push_py_fn_metrics_copies_all_function_metric_buckets() {
        let metrics = crate::py_metrics::FunctionMetrics {
            statements: 2,
            arguments_positional: 3,
            arguments_keyword_only: 4,
            max_indentation: 5,
            nested_function_depth: 6,
            returns: 7,
            max_return_values: 8,
            branches: 9,
            local_variables: 10,
            max_try_block_statements: 11,
            boolean_parameters: 12,
            decorators: 13,
            calls: 14,
            ..Default::default()
        };
        let mut stats = MetricStats::default();
        push_py_fn_metrics(&mut stats, &metrics);

        assert_eq!(stats.statements_per_function, vec![2]);
        assert_eq!(stats.arguments_positional, vec![3]);
        assert_eq!(stats.arguments_keyword_only, vec![4]);
        assert_eq!(stats.max_indentation, vec![5]);
        assert_eq!(stats.nested_function_depth, vec![6]);
        assert_eq!(stats.returns_per_function, vec![7]);
        assert_eq!(stats.return_values_per_function, vec![8]);
        assert_eq!(stats.branches_per_function, vec![9]);
        assert_eq!(stats.local_variables_per_function, vec![10]);
        assert_eq!(stats.statements_per_try_block, vec![11]);
        assert_eq!(stats.boolean_parameters, vec![12]);
        assert_eq!(stats.annotations_per_function, vec![13]);
        assert_eq!(stats.calls_per_function, vec![14]);
    }
}
