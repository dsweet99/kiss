//! Statistics collection and percentile calculation for metrics

use crate::counts::{compute_class_metrics, compute_file_metrics, compute_function_metrics};
use crate::graph::{compute_cyclomatic_complexity, DependencyGraph};
use crate::parsing::ParsedFile;
use tree_sitter::Node;

/// All metric values collected from a codebase
#[derive(Debug, Default)]
pub struct MetricStats {
    pub statements_per_function: Vec<usize>,
    pub arguments_per_function: Vec<usize>,
    pub arguments_positional: Vec<usize>,
    pub arguments_keyword_only: Vec<usize>,
    pub max_indentation: Vec<usize>,
    pub nested_function_depth: Vec<usize>,
    pub returns_per_function: Vec<usize>,
    pub branches_per_function: Vec<usize>,
    pub local_variables_per_function: Vec<usize>,
    pub cyclomatic_complexity: Vec<usize>,
    pub methods_per_class: Vec<usize>,
    pub lines_per_file: Vec<usize>,
    pub classes_per_file: Vec<usize>,
    pub imports_per_file: Vec<usize>,
    // Graph metrics (per module)
    pub fan_out: Vec<usize>,
    pub fan_in: Vec<usize>,
    pub instability: Vec<usize>, // Stored as percentage (0-100)
}

impl MetricStats {
    /// Collect metrics from all parsed files
    pub fn collect(parsed_files: &[&ParsedFile]) -> Self {
        let mut stats = Self::default();

        for parsed in parsed_files {
            // File-level metrics
            let file_metrics = compute_file_metrics(parsed);
            stats.lines_per_file.push(file_metrics.lines);
            stats.classes_per_file.push(file_metrics.classes);
            stats.imports_per_file.push(file_metrics.imports);

            // Walk AST for function and class metrics
            collect_from_node(
                parsed.tree.root_node(),
                &parsed.source,
                &mut stats,
                false,
            );
        }

        stats
    }

    /// Merge another MetricStats into this one
    pub fn merge(&mut self, other: MetricStats) {
        self.statements_per_function.extend(other.statements_per_function);
        self.arguments_per_function.extend(other.arguments_per_function);
        self.arguments_positional.extend(other.arguments_positional);
        self.arguments_keyword_only.extend(other.arguments_keyword_only);
        self.max_indentation.extend(other.max_indentation);
        self.nested_function_depth.extend(other.nested_function_depth);
        self.returns_per_function.extend(other.returns_per_function);
        self.branches_per_function.extend(other.branches_per_function);
        self.local_variables_per_function.extend(other.local_variables_per_function);
        self.cyclomatic_complexity.extend(other.cyclomatic_complexity);
        self.methods_per_class.extend(other.methods_per_class);
        self.lines_per_file.extend(other.lines_per_file);
        self.classes_per_file.extend(other.classes_per_file);
        self.imports_per_file.extend(other.imports_per_file);
        self.fan_out.extend(other.fan_out);
        self.fan_in.extend(other.fan_in);
        self.instability.extend(other.instability);
    }

    /// Collect graph metrics from a dependency graph
    pub fn collect_graph_metrics(&mut self, graph: &DependencyGraph) {
        for module_name in graph.nodes.keys() {
            let metrics = graph.module_metrics(module_name);
            self.fan_out.push(metrics.fan_out);
            self.fan_in.push(metrics.fan_in);
            // Store instability as percentage (0-100)
            self.instability.push((metrics.instability * 100.0).round() as usize);
        }
    }
}

fn collect_from_node(node: Node, source: &str, stats: &mut MetricStats, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let metrics = compute_function_metrics(node, source);
            stats.statements_per_function.push(metrics.statements);
            stats.arguments_per_function.push(metrics.arguments);
            stats.arguments_positional.push(metrics.arguments_positional);
            stats.arguments_keyword_only.push(metrics.arguments_keyword_only);
            stats.max_indentation.push(metrics.max_indentation);
            stats.nested_function_depth.push(metrics.nested_function_depth);
            stats.returns_per_function.push(metrics.returns);
            stats.branches_per_function.push(metrics.branches);
            stats.local_variables_per_function.push(metrics.local_variables);

            let complexity = compute_cyclomatic_complexity(node);
            stats.cyclomatic_complexity.push(complexity);

            // Recurse into function body for nested functions
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_from_node(child, source, stats, false);
                }
            }
        }
        "class_definition" => {
            let metrics = compute_class_metrics(node);
            stats.methods_per_class.push(metrics.methods);

            // Recurse into class body
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_from_node(child, source, stats, true);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_from_node(child, source, stats, inside_class);
                }
            }
        }
    }
}

/// Calculate percentile value from a sorted slice
pub fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Summary statistics for a single metric
#[derive(Debug)]
pub struct PercentileSummary {
    pub name: &'static str,
    pub count: usize,
    pub p50: usize,
    pub p90: usize,
    pub p95: usize,
    pub p99: usize,
    pub max: usize,
}

impl PercentileSummary {
    /// Compute percentile summary from a list of values
    pub fn from_values(name: &'static str, values: &[usize]) -> Self {
        if values.is_empty() {
            return Self {
                name,
                count: 0,
                p50: 0,
                p90: 0,
                p95: 0,
                p99: 0,
                max: 0,
            };
        }

        let mut sorted = values.to_vec();
        sorted.sort_unstable();

        Self {
            name,
            count: sorted.len(),
            p50: percentile(&sorted, 50.0),
            p90: percentile(&sorted, 90.0),
            p95: percentile(&sorted, 95.0),
            p99: percentile(&sorted, 99.0),
            max: *sorted.last().unwrap_or(&0),
        }
    }
}

/// Generate all percentile summaries from collected stats
pub fn compute_summaries(stats: &MetricStats) -> Vec<PercentileSummary> {
    vec![
        PercentileSummary::from_values("Statements per function", &stats.statements_per_function),
        PercentileSummary::from_values("Arguments (total)", &stats.arguments_per_function),
        PercentileSummary::from_values("Arguments (positional)", &stats.arguments_positional),
        PercentileSummary::from_values("Arguments (keyword-only)", &stats.arguments_keyword_only),
        PercentileSummary::from_values("Max indentation depth", &stats.max_indentation),
        PercentileSummary::from_values("Nested function depth", &stats.nested_function_depth),
        PercentileSummary::from_values("Returns per function", &stats.returns_per_function),
        PercentileSummary::from_values("Branches per function", &stats.branches_per_function),
        PercentileSummary::from_values("Local variables per function", &stats.local_variables_per_function),
        PercentileSummary::from_values("Cyclomatic complexity", &stats.cyclomatic_complexity),
        PercentileSummary::from_values("Methods per class", &stats.methods_per_class),
        PercentileSummary::from_values("Lines per file", &stats.lines_per_file),
        PercentileSummary::from_values("Classes per file", &stats.classes_per_file),
        PercentileSummary::from_values("Imports per file", &stats.imports_per_file),
        PercentileSummary::from_values("Fan-out (per module)", &stats.fan_out),
        PercentileSummary::from_values("Fan-in (per module)", &stats.fan_in),
        PercentileSummary::from_values("Instability % (per module)", &stats.instability),
    ]
}

/// Format summaries as a table string
pub fn format_stats_table(summaries: &[PercentileSummary]) -> String {
    let mut output = String::new();
    
    // Header
    output.push_str(&format!(
        "{:<32} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
        "Metric", "Count", "50%", "90%", "95%", "99%", "Max"
    ));
    output.push_str(&"-".repeat(74));
    output.push('\n');

    // Rows
    for s in summaries {
        if s.count > 0 {
            output.push_str(&format!(
                "{:<32} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
                s.name, s.count, s.p50, s.p90, s.p95, s.p99, s.max
            ));
        }
    }

    output
}

/// Generate a config TOML string using 99th percentile values
pub fn generate_config_toml(summaries: &[PercentileSummary]) -> String {
    let mut output = String::new();
    output.push_str("# Generated by kiss mimic\n");
    output.push_str("# Thresholds based on 99th percentile of analyzed codebases\n\n");
    output.push_str("[thresholds]\n");

    for s in summaries {
        let key = match s.name {
            "Statements per function" => "statements_per_function",
            "Arguments (total)" => "arguments_per_function",
            "Arguments (positional)" => "arguments_positional",
            "Arguments (keyword-only)" => "arguments_keyword_only",
            "Max indentation depth" => "max_indentation_depth",
            "Nested function depth" => "nested_function_depth",
            "Returns per function" => "returns_per_function",
            "Branches per function" => "branches_per_function",
            "Local variables per function" => "local_variables_per_function",
            "Cyclomatic complexity" => "cyclomatic_complexity",
            "Methods per class" => "methods_per_class",
            "Lines per file" => "lines_per_file",
            "Classes per file" => "classes_per_file",
            "Imports per file" => "imports_per_file",
            "Fan-out (per module)" => "fan_out",
            "Fan-in (per module)" => "fan_in",
            // Instability is informational, not a threshold
            _ => continue,
        };
        output.push_str(&format!("{} = {}\n", key, s.p99));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_empty_returns_zero() {
        assert_eq!(percentile(&[], 50.0), 0);
        assert_eq!(percentile(&[], 99.0), 0);
    }

    #[test]
    fn percentile_single_element() {
        assert_eq!(percentile(&[42], 0.0), 42);
        assert_eq!(percentile(&[42], 50.0), 42);
        assert_eq!(percentile(&[42], 100.0), 42);
    }

    #[test]
    fn percentile_multiple_elements() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&data, 0.0), 1);
        // 50th percentile of 10 elements: idx = (9 * 0.5).round() = 5, so data[5] = 6
        assert_eq!(percentile(&data, 50.0), 6);
        assert_eq!(percentile(&data, 100.0), 10);
    }

    #[test]
    fn summary_from_empty_values() {
        let summary = PercentileSummary::from_values("test", &[]);
        assert_eq!(summary.count, 0);
        assert_eq!(summary.p50, 0);
        assert_eq!(summary.max, 0);
    }

    #[test]
    fn summary_from_values_computes_percentiles() {
        let values: Vec<usize> = (1..=100).collect();
        let summary = PercentileSummary::from_values("test", &values);
        assert_eq!(summary.count, 100);
        // 50th percentile: idx = (99 * 0.5).round() = 50, so data[50] = 51
        assert_eq!(summary.p50, 51);
        assert_eq!(summary.p90, 90);
        assert_eq!(summary.p95, 95);
        assert_eq!(summary.p99, 99);
        assert_eq!(summary.max, 100);
    }

    #[test]
    fn generate_config_toml_produces_valid_toml() {
        let summaries = vec![
            PercentileSummary {
                name: "Statements per function",
                count: 10,
                p50: 5,
                p90: 9,
                p95: 10,
                p99: 15,
                max: 20,
            },
        ];
        let toml = generate_config_toml(&summaries);
        assert!(toml.contains("[thresholds]"));
        assert!(toml.contains("statements_per_function = 15"));
    }

    #[test]
    fn format_stats_table_includes_header() {
        let summaries = vec![
            PercentileSummary {
                name: "Test metric",
                count: 5,
                p50: 1,
                p90: 2,
                p95: 3,
                p99: 4,
                max: 5,
            },
        ];
        let table = format_stats_table(&summaries);
        assert!(table.contains("Metric"));
        assert!(table.contains("50%"));
        assert!(table.contains("99%"));
        assert!(table.contains("Test metric"));
    }
}

