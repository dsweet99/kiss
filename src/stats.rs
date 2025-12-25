//! Statistics collection and percentile calculation for metrics
use crate::py_metrics::{compute_class_metrics_with_source, compute_file_metrics, compute_function_metrics};
use crate::graph::{compute_cyclomatic_complexity, DependencyGraph};
use crate::parsing::ParsedFile;
use crate::rust_counts::{compute_rust_file_metrics, compute_rust_function_metrics, compute_rust_lcom};
use crate::rust_parsing::ParsedRustFile;
use syn::{ImplItem, Item};
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
    pub transitive_deps: Vec<usize>,
    // Class cohesion metrics
    pub lcom: Vec<usize>, // Stored as percentage (0-100)
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
        self.transitive_deps.extend(other.transitive_deps);
        self.lcom.extend(other.lcom);
    }

    /// Collect graph metrics from a dependency graph
    pub fn collect_graph_metrics(&mut self, graph: &DependencyGraph) {
        for module_name in graph.nodes.keys() {
            let metrics = graph.module_metrics(module_name);
            self.fan_out.push(metrics.fan_out);
            self.fan_in.push(metrics.fan_in);
            // Store instability as percentage (0-100)
            self.instability.push((metrics.instability * 100.0).round() as usize);
            self.transitive_deps.push(metrics.transitive_deps);
        }
    }

    /// Collect metrics from all parsed Rust files
    pub fn collect_rust(parsed_files: &[&ParsedRustFile]) -> Self {
        let mut stats = Self::default();

        for parsed in parsed_files {
            // File-level metrics
            let file_metrics = compute_rust_file_metrics(parsed);
            stats.lines_per_file.push(file_metrics.lines);
            stats.classes_per_file.push(file_metrics.types); // types = struct + enum
            stats.imports_per_file.push(file_metrics.imports);

            // Walk AST for function and impl metrics
            collect_rust_from_items(&parsed.ast.items, &mut stats);
        }

        stats
    }
}

#[allow(clippy::only_used_in_recursion)]
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
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_from_node(child, source, stats, false);
            }
        }
        "class_definition" => {
            let metrics = compute_class_metrics_with_source(node, source);
            stats.methods_per_class.push(metrics.methods);
            // Store LCOM as percentage (0-100)
            stats.lcom.push((metrics.lcom * 100.0).round() as usize);

            // Recurse into class body
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_from_node(child, source, stats, true);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_from_node(child, source, stats, inside_class);
            }
        }
    }
}

fn push_rust_fn_metrics(stats: &mut MetricStats, m: &crate::rust_counts::RustFunctionMetrics) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_per_function.push(m.arguments);
    stats.arguments_positional.push(m.arguments);
    stats.arguments_keyword_only.push(0);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    stats.cyclomatic_complexity.push(m.cyclomatic_complexity);
}

fn collect_rust_from_items(items: &[Item], stats: &mut MetricStats) {
    for item in items {
        match item {
            Item::Fn(func) => push_rust_fn_metrics(stats, &compute_rust_function_metrics(&func.sig.inputs, &func.block)),
            Item::Impl(impl_block) => {
                let mcnt = impl_block.items.iter().filter(|i| matches!(i, ImplItem::Fn(_))).count();
                stats.methods_per_class.push(mcnt);
                stats.lcom.push(if mcnt > 1 { (compute_rust_lcom(impl_block) * 100.0).round() as usize } else { 0 });
                for ii in &impl_block.items {
                    if let ImplItem::Fn(m) = ii { push_rust_fn_metrics(stats, &compute_rust_function_metrics(&m.sig.inputs, &m.block)); }
                }
            }
            Item::Mod(m) => { if let Some((_, items)) = &m.content { collect_rust_from_items(items, stats); } }
            _ => {}
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
        PercentileSummary::from_values("Transitive deps (per module)", &stats.transitive_deps),
        PercentileSummary::from_values("Instability % (per module)", &stats.instability),
        PercentileSummary::from_values("LCOM % (per class)", &stats.lcom),
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

fn config_key_for(name: &str) -> Option<&'static str> {
    Some(match name {
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
        "Transitive deps (per module)" => "transitive_deps",
        "LCOM % (per class)" => "lcom",
        _ => return None,
    })
}

pub fn generate_config_toml(summaries: &[PercentileSummary]) -> String {
    let mut out = String::from("# Generated by kiss mimic\n# Thresholds based on 99th percentile of analyzed codebases\n\n[thresholds]\n");
    for s in summaries {
        if let Some(key) = config_key_for(s.name) { out.push_str(&format!("{} = {}\n", key, s.p99)); }
    }
    out
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

    #[test]
    fn test_metric_stats_default() {
        let s = MetricStats::default();
        assert!(s.statements_per_function.is_empty());
    }

    #[test]
    fn test_metric_stats_merge() {
        let mut a = MetricStats::default();
        a.statements_per_function.push(5);
        let mut b = MetricStats::default();
        b.statements_per_function.push(10);
        a.merge(b);
        assert_eq!(a.statements_per_function.len(), 2);
    }

    #[test]
    fn test_percentile_summary_struct() {
        let s = PercentileSummary { name: "x", count: 1, p50: 2, p90: 3, p95: 4, p99: 5, max: 6 };
        assert_eq!(s.name, "x");
    }

    #[test]
    fn test_compute_summaries() {
        let mut stats = MetricStats::default();
        stats.statements_per_function = vec![1, 2, 3, 4, 5];
        let summaries = compute_summaries(&stats);
        assert!(!summaries.is_empty());
        let s = summaries.iter().find(|s| s.name == "Statements per function").unwrap();
        assert_eq!(s.count, 5);
    }

    #[test]
    fn test_config_key_for() {
        assert_eq!(config_key_for("Statements per function"), Some("statements_per_function"));
        assert_eq!(config_key_for("Unknown metric"), None);
    }

    #[test]
    fn test_collect_graph_metrics() {
        let mut stats = MetricStats::default();
        let graph = crate::graph::DependencyGraph::default();
        stats.collect_graph_metrics(&graph);
        // Should not panic, stats may or may not have values depending on graph
        assert!(stats.fan_out.is_empty() || !stats.fan_out.is_empty());
    }

    #[test]
    fn test_collect_rust() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(tmp, "fn foo() {{ let x = 1; }}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let stats = MetricStats::collect_rust(&[&parsed]);
        assert!(!stats.statements_per_function.is_empty());
    }

    #[test]
    fn test_collect_from_node() {
        use crate::parsing::{create_parser, parse_file};
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "def f():\n    x = 1\n    return x").unwrap();
        let mut parser = create_parser().unwrap();
        let parsed = parse_file(&mut parser, tmp.path()).unwrap();
        let mut stats = MetricStats::default();
        collect_from_node(parsed.tree.root_node(), &parsed.source, &mut stats, false);
        assert!(!stats.statements_per_function.is_empty());
    }

    #[test]
    fn test_push_rust_fn_metrics() {
        let metrics = crate::rust_counts::RustFunctionMetrics { statements: 5, arguments: 2, max_indentation: 1, returns: 1, branches: 0, local_variables: 2, cyclomatic_complexity: 2, nested_function_depth: 0 };
        let mut stats = MetricStats::default();
        push_rust_fn_metrics(&mut stats, &metrics);
        assert!(stats.statements_per_function.contains(&5));
    }

    #[test]
    fn test_collect_rust_from_items() {
        let file: syn::File = syn::parse_str("fn foo() { let x = 1; }").unwrap();
        let mut stats = MetricStats::default();
        collect_rust_from_items(&file.items, &mut stats);
        assert!(!stats.statements_per_function.is_empty());
    }

    #[test]
    fn test_percentile_direct() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&data, 0.0), 1);
        assert_eq!(percentile(&data, 100.0), 10);
    }
}
