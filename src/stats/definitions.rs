#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricScope {
    Function,
    Type,
    File,
    Module,
}

#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub metric_id: &'static str,
    pub scope: MetricScope,
}

/// Central registry of all metrics with stable IDs
pub const METRICS: &[MetricDef] = &[
    MetricDef {
        metric_id: "statements_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "arguments_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "positional_args",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "keyword_only_args",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "max_indentation_depth",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "nested_function_depth",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "returns_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "return_values_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "branches_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "local_variables_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "statements_per_try_block",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "boolean_parameters",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "annotations_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "calls_per_function",
        scope: MetricScope::Function,
    },
    MetricDef {
        metric_id: "methods_per_class",
        scope: MetricScope::Type,
    },
    MetricDef {
        metric_id: "statements_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "lines_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "functions_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "interface_types_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "concrete_types_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "imported_names_per_file",
        scope: MetricScope::File,
    },
    MetricDef {
        // Per-file *un*covered percentage (100 - coverage). Stored inverted so
        // it matches the convention of every other metric in this registry —
        // higher = worse — and so the upper-percentile columns (`p90`/`p95`/
        // `p99`/`max`) shown by `format_stats_table` highlight the *worst*-
        // covered files instead of the best-covered ones.
        metric_id: "inv_test_coverage",
        scope: MetricScope::File,
    },
    MetricDef {
        metric_id: "fan_in",
        scope: MetricScope::Module,
    },
    MetricDef {
        metric_id: "fan_out",
        scope: MetricScope::Module,
    },
    MetricDef {
        metric_id: "cycle_size",
        scope: MetricScope::Module,
    },
    MetricDef {
        metric_id: "indirect_dependencies",
        scope: MetricScope::Module,
    },
    MetricDef {
        metric_id: "dependency_depth",
        scope: MetricScope::Module,
    },
];

pub fn get_metric_def(metric_id: &str) -> Option<&'static MetricDef> {
    METRICS.iter().find(|m| m.metric_id == metric_id)
}
