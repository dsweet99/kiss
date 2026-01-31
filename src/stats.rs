
use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{compute_class_metrics, compute_file_metrics, compute_function_metrics};
use crate::rust_fn_metrics::{compute_rust_file_metrics, compute_rust_function_metrics};
use crate::rust_parsing::ParsedRustFile;
use syn::{ImplItem, Item};
use tree_sitter::Node;

#[derive(Debug, Default)]
pub struct MetricStats {
    pub statements_per_function: Vec<usize>,
    pub arguments_per_function: Vec<usize>,
    pub arguments_positional: Vec<usize>,
    pub arguments_keyword_only: Vec<usize>,
    pub max_indentation: Vec<usize>,
    pub nested_function_depth: Vec<usize>,
    pub returns_per_function: Vec<usize>,
    pub return_values_per_function: Vec<usize>,
    pub branches_per_function: Vec<usize>,
    pub local_variables_per_function: Vec<usize>,
    pub statements_per_try_block: Vec<usize>,
    pub boolean_parameters: Vec<usize>,
    pub annotations_per_function: Vec<usize>,
    pub calls_per_function: Vec<usize>,
    pub methods_per_class: Vec<usize>,
    pub statements_per_file: Vec<usize>,
    pub lines_per_file: Vec<usize>,
    pub functions_per_file: Vec<usize>,
    pub interface_types_per_file: Vec<usize>,
    pub concrete_types_per_file: Vec<usize>,
    pub imported_names_per_file: Vec<usize>,
    pub fan_in: Vec<usize>,
    pub fan_out: Vec<usize>,
    pub cycle_size: Vec<usize>,
    pub transitive_dependencies: Vec<usize>,
    pub dependency_depth: Vec<usize>,
}

impl MetricStats {
    pub fn collect(parsed_files: &[&ParsedFile]) -> Self {
        let mut stats = Self::default();
        for parsed in parsed_files {
            let fm = compute_file_metrics(parsed);
            stats.statements_per_file.push(fm.statements);
            stats.lines_per_file.push(parsed.source.lines().count());
            stats.functions_per_file.push(fm.functions);
            stats.interface_types_per_file.push(fm.interface_types);
            stats.concrete_types_per_file.push(fm.concrete_types);
            stats.imported_names_per_file.push(fm.imports);
            collect_from_node(parsed.tree.root_node(), &parsed.source, &mut stats, false);
        }
        stats
    }

    pub fn merge(&mut self, o: Self) {
        macro_rules! ext { ($($f:ident),*) => { $(self.$f.extend(o.$f);)* }; }
        ext!(statements_per_function, arguments_per_function, arguments_positional, arguments_keyword_only, max_indentation, nested_function_depth, returns_per_function, return_values_per_function, branches_per_function, local_variables_per_function, statements_per_try_block, boolean_parameters, annotations_per_function, calls_per_function, methods_per_class, statements_per_file, lines_per_file, functions_per_file, interface_types_per_file, concrete_types_per_file, imported_names_per_file, fan_in, fan_out, cycle_size, transitive_dependencies, dependency_depth);
    }

    pub fn collect_graph_metrics(&mut self, graph: &DependencyGraph) {
        for name in graph.nodes.keys() {
            let m = graph.module_metrics(name);
            self.fan_in.push(m.fan_in);
            self.fan_out.push(m.fan_out);
            self.transitive_dependencies.push(m.transitive_dependencies);
            self.dependency_depth.push(m.dependency_depth);
        }
        let max_cycle = graph.find_cycles().cycles.iter().map(Vec::len).max().unwrap_or(0);
        self.cycle_size.push(max_cycle);
    }

    pub fn max_depth(&self) -> usize {
        self.dependency_depth.iter().copied().max().unwrap_or(0)
    }

    pub fn collect_rust(parsed_files: &[&ParsedRustFile]) -> Self {
        let mut stats = Self::default();
        for parsed in parsed_files {
            let fm = compute_rust_file_metrics(parsed);
            stats.statements_per_file.push(fm.statements);
            stats.lines_per_file.push(parsed.source.lines().count());
            stats.functions_per_file.push(fm.functions);
            stats.interface_types_per_file.push(fm.interface_types);
            stats.concrete_types_per_file.push(fm.concrete_types);
            stats.imported_names_per_file.push(fm.imports);
            collect_rust_from_items(&parsed.ast.items, &mut stats);
        }
        stats
    }
}

// inside_class tracks context for method counting; passed through recursion to nested scopes
#[allow(clippy::only_used_in_recursion)]
fn collect_from_node(node: Node, source: &str, stats: &mut MetricStats, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            push_py_fn_metrics(stats, &compute_function_metrics(node, source));
            let mut c = node.walk();
            for child in node.children(&mut c) { collect_from_node(child, source, stats, false); }
        }
        "class_definition" => {
            let m = compute_class_metrics(node);
            stats.methods_per_class.push(m.methods);
            let mut c = node.walk();
            for child in node.children(&mut c) { collect_from_node(child, source, stats, true); }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c) { collect_from_node(child, source, stats, inside_class); }
        }
    }
}

fn push_py_fn_metrics(stats: &mut MetricStats, m: &crate::py_metrics::FunctionMetrics) {
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
    stats.statements_per_try_block.push(m.max_try_block_statements);
    stats.boolean_parameters.push(m.boolean_parameters);
    stats.annotations_per_function.push(m.decorators);
    stats.calls_per_function.push(m.calls);
}

fn push_rust_fn_metrics(stats: &mut MetricStats, m: &crate::rust_counts::RustFunctionMetrics) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_per_function.push(m.arguments);
    stats.arguments_positional.push(m.arguments);
    stats.arguments_keyword_only.push(0);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    // N/A: Rust doesn't have multiple-return-value tuples in the same sense as Python
    stats.return_values_per_function.push(0);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    // N/A: try-block size is Python-only
    stats.statements_per_try_block.push(0);
    stats.boolean_parameters.push(m.bool_parameters);
    stats.annotations_per_function.push(m.attributes);
    stats.calls_per_function.push(m.calls);
}

fn collect_rust_from_items(items: &[Item], stats: &mut MetricStats) {
    for item in items {
        match item {
            Item::Fn(f) => push_rust_fn_metrics(stats, &compute_rust_function_metrics(&f.sig.inputs, &f.block, f.attrs.len())),
            Item::Impl(i) => {
                let mcnt = i.items.iter().filter(|ii| matches!(ii, ImplItem::Fn(_))).count();
                stats.methods_per_class.push(mcnt);
                for ii in &i.items { if let ImplItem::Fn(m) = ii { push_rust_fn_metrics(stats, &compute_rust_function_metrics(&m.sig.inputs, &m.block, m.attrs.len())); } }
            }
            Item::Mod(m) => if let Some((_, items)) = &m.content { collect_rust_from_items(items, stats); },
            _ => {}
        }
    }
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() { return 0 }
    let len = sorted.len();
    let idx_f = (len.saturating_sub(1) as f64) * p / 100.0;
    let idx = idx_f.round().max(0.0) as usize;
    sorted[idx.min(len - 1)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricScope { Function, Type, File, Module }

#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub metric_id: &'static str,
    pub display_name: &'static str,
    pub scope: MetricScope,
}

/// Central registry of all metrics with stable IDs
pub const METRICS: &[MetricDef] = &[
    MetricDef { metric_id: "statements_per_function", display_name: "Statements per function", scope: MetricScope::Function },
    MetricDef { metric_id: "args_total", display_name: "Arguments (total)", scope: MetricScope::Function },
    MetricDef { metric_id: "args_positional", display_name: "Arguments (positional)", scope: MetricScope::Function },
    MetricDef { metric_id: "args_keyword_only", display_name: "Arguments (keyword-only)", scope: MetricScope::Function },
    MetricDef { metric_id: "max_indentation_depth", display_name: "Max indentation depth", scope: MetricScope::Function },
    MetricDef { metric_id: "nested_function_depth", display_name: "Nested function depth", scope: MetricScope::Function },
    MetricDef { metric_id: "returns_per_function", display_name: "Returns per function", scope: MetricScope::Function },
    MetricDef { metric_id: "return_values_per_return", display_name: "Return values per return", scope: MetricScope::Function },
    MetricDef { metric_id: "branches_per_function", display_name: "Branches per function", scope: MetricScope::Function },
    MetricDef { metric_id: "local_variables_per_function", display_name: "Local variables per function", scope: MetricScope::Function },
    MetricDef { metric_id: "statements_per_try_block", display_name: "Statements per try block", scope: MetricScope::Function },
    MetricDef { metric_id: "boolean_parameters", display_name: "Boolean parameters", scope: MetricScope::Function },
    MetricDef { metric_id: "annotations_per_function", display_name: "Annotations per function", scope: MetricScope::Function },
    MetricDef { metric_id: "calls_per_function", display_name: "Calls per function", scope: MetricScope::Function },
    MetricDef { metric_id: "methods_per_type", display_name: "Methods per type", scope: MetricScope::Type },
    MetricDef { metric_id: "statements_per_file", display_name: "Statements per file", scope: MetricScope::File },
    MetricDef { metric_id: "lines_per_file", display_name: "Lines per file", scope: MetricScope::File },
    MetricDef { metric_id: "functions_per_file", display_name: "Functions per file", scope: MetricScope::File },
    MetricDef { metric_id: "interface_types_per_file", display_name: "Interface types per file", scope: MetricScope::File },
    MetricDef { metric_id: "concrete_types_per_file", display_name: "Concrete types per file", scope: MetricScope::File },
    MetricDef { metric_id: "imported_names_per_file", display_name: "Imported names per file", scope: MetricScope::File },
    MetricDef { metric_id: "fan_in", display_name: "Fan-in (per module)", scope: MetricScope::Module },
    MetricDef { metric_id: "fan_out", display_name: "Fan-out (per module)", scope: MetricScope::Module },
    MetricDef { metric_id: "cycle_size", display_name: "Cycle size (modules)", scope: MetricScope::Module },
    MetricDef { metric_id: "transitive_deps", display_name: "Transitive deps (per module)", scope: MetricScope::Module },
    MetricDef { metric_id: "dependency_depth", display_name: "Dependency depth (per module)", scope: MetricScope::Module },
];

pub fn get_metric_def(metric_id: &str) -> Option<&'static MetricDef> {
    METRICS.iter().find(|m| m.metric_id == metric_id)
}

#[derive(Debug)]
pub struct PercentileSummary {
    pub metric_id: &'static str,
    pub display_name: &'static str,
    pub count: usize, pub p50: usize, pub p90: usize, pub p95: usize, pub p99: usize, pub max: usize,
}

impl PercentileSummary {
    pub fn from_values(metric_id: &'static str, display_name: &'static str, values: &[usize]) -> Self {
        if values.is_empty() { return Self { metric_id, display_name, count: 0, p50: 0, p90: 0, p95: 0, p99: 0, max: 0 } }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        Self { metric_id, display_name, count: sorted.len(), p50: percentile(&sorted, 50.0), p90: percentile(&sorted, 90.0), 
               p95: percentile(&sorted, 95.0), p99: percentile(&sorted, 99.0), max: *sorted.last().unwrap_or(&0) }
    }
}

pub fn compute_summaries(stats: &MetricStats) -> Vec<PercentileSummary> {
    vec![
        PercentileSummary::from_values("statements_per_function", "Statements per function", &stats.statements_per_function),
        PercentileSummary::from_values("args_total", "Arguments (total)", &stats.arguments_per_function),
        PercentileSummary::from_values("args_positional", "Arguments (positional)", &stats.arguments_positional),
        PercentileSummary::from_values("args_keyword_only", "Arguments (keyword-only)", &stats.arguments_keyword_only),
        PercentileSummary::from_values("max_indentation_depth", "Max indentation depth", &stats.max_indentation),
        PercentileSummary::from_values("nested_function_depth", "Nested function depth", &stats.nested_function_depth),
        PercentileSummary::from_values("returns_per_function", "Returns per function", &stats.returns_per_function),
        PercentileSummary::from_values("return_values_per_return", "Return values per return", &stats.return_values_per_function),
        PercentileSummary::from_values("branches_per_function", "Branches per function", &stats.branches_per_function),
        PercentileSummary::from_values("local_variables_per_function", "Local variables per function", &stats.local_variables_per_function),
        PercentileSummary::from_values("statements_per_try_block", "Statements per try block", &stats.statements_per_try_block),
        PercentileSummary::from_values("boolean_parameters", "Boolean parameters", &stats.boolean_parameters),
        PercentileSummary::from_values("annotations_per_function", "Annotations per function", &stats.annotations_per_function),
        PercentileSummary::from_values("calls_per_function", "Calls per function", &stats.calls_per_function),
        PercentileSummary::from_values("methods_per_type", "Methods per type", &stats.methods_per_class),
        PercentileSummary::from_values("statements_per_file", "Statements per file", &stats.statements_per_file),
        PercentileSummary::from_values("lines_per_file", "Lines per file", &stats.lines_per_file),
        PercentileSummary::from_values("functions_per_file", "Functions per file", &stats.functions_per_file),
        PercentileSummary::from_values("interface_types_per_file", "Interface types per file", &stats.interface_types_per_file),
        PercentileSummary::from_values("concrete_types_per_file", "Concrete types per file", &stats.concrete_types_per_file),
        PercentileSummary::from_values("imported_names_per_file", "Imported names per file", &stats.imported_names_per_file),
        PercentileSummary::from_values("fan_in", "Fan-in (per module)", &stats.fan_in),
        PercentileSummary::from_values("fan_out", "Fan-out (per module)", &stats.fan_out),
        PercentileSummary::from_values("cycle_size", "Cycle size (modules)", &stats.cycle_size),
        PercentileSummary::from_values("transitive_deps", "Transitive deps (per module)", &stats.transitive_dependencies),
        PercentileSummary::from_values("dependency_depth", "Dependency depth (per module)", &stats.dependency_depth),
    ]
}

pub fn format_stats_table(summaries: &[PercentileSummary]) -> String {
    use std::fmt::Write;
    let mut out = format!("{:<28} {:<32} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}\n", 
        "metric_id", "display_name", "N", "p50", "p90", "p95", "p99", "max");
    out.push_str(&"-".repeat(100));
    out.push('\n');
    for s in summaries.iter().filter(|s| s.count > 0) {
        let _ = writeln!(out, "{:<28} {:<32} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}", 
            s.metric_id, s.display_name, s.count, s.p50, s.p90, s.p95, s.p99, s.max);
    }
    out
}

/// Map `metric_id` to config key (some metrics use different config key names)
fn config_key_for(metric_id: &str) -> Option<&'static str> {
    Some(match metric_id {
        "statements_per_function" => "statements_per_function",
        "args_total" => "arguments_per_function",
        "args_positional" => "arguments_positional",
        "args_keyword_only" => "arguments_keyword_only",
        "max_indentation_depth" => "max_indentation_depth",
        "nested_function_depth" => "nested_function_depth",
        "returns_per_function" => "returns_per_function",
        "branches_per_function" => "branches_per_function",
        "local_variables_per_function" => "local_variables_per_function",
        "methods_per_type" => "methods_per_class",
        "statements_per_file" => "statements_per_file",
        "functions_per_file" => "functions_per_file",
        "interface_types_per_file" => "interface_types_per_file",
        "concrete_types_per_file" => "concrete_types_per_file",
        "imported_names_per_file" => "imported_names_per_file",
        _ => return None,
    })
}

pub fn generate_config_toml(summaries: &[PercentileSummary]) -> String {
    use std::fmt::Write;
    let mut out = String::from("# Generated by kiss mimic\n# Thresholds based on 99th percentile\n\n[thresholds]\n");
    for s in summaries { if let Some(k) = config_key_for(s.metric_id) { let _ = writeln!(out, "{k} = {}", s.p99); } }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write;

    #[test]
    fn test_stats_helpers() {
        assert_eq!(percentile(&[], 50.0), 0);
        assert_eq!(percentile(&[42], 50.0), 42);
        let s = PercentileSummary::from_values("test_id", "Test Name", &[]);
        assert_eq!(s.count, 0);
        let vals: Vec<usize> = (1..=100).collect();
        assert_eq!(PercentileSummary::from_values("test_id", "Test Name", &vals).max, 100);
        let mut a = MetricStats::default();
        a.statements_per_function.push(5);
        let mut b = MetricStats::default();
        b.statements_per_function.push(10);
        a.merge(b);
        assert_eq!(a.statements_per_function.len(), 2);
        let s2 = MetricStats { statements_per_function: vec![1, 2, 3], ..Default::default() };
        assert!(!compute_summaries(&s2).is_empty());
        let toml = generate_config_toml(&[PercentileSummary { metric_id: "statements_per_function", display_name: "Statements per function", count: 10, p50: 5, p90: 9, p95: 10, p99: 15, max: 20 }]);
        assert!(toml.contains("statements_per_function = 15"));
        assert_eq!(config_key_for("statements_per_function"), Some("statements_per_function"));
        assert!(format_stats_table(&[PercentileSummary { metric_id: "test_id", display_name: "Test Name", count: 10, p50: 5, p90: 8, p95: 9, p99: 10, max: 12 }]).contains("Test Name"));
    }

    #[test]
    fn test_metric_registry() {
        assert!(get_metric_def("statements_per_function").is_some());
        assert!(get_metric_def("fan_in").is_some());
        assert!(get_metric_def("nonexistent").is_none());
        assert_eq!(get_metric_def("statements_per_function").unwrap().scope, MetricScope::Function);
        assert_eq!(get_metric_def("fan_in").unwrap().scope, MetricScope::Module);
        assert!(METRICS.len() > 20); // Verify we have a reasonable number of metrics
        // Test MetricDef struct fields
        let def = MetricDef { metric_id: "test", display_name: "Test", scope: MetricScope::Function };
        assert_eq!(def.metric_id, "test");
    }

    #[test]
    fn test_push_py_fn_metrics() {
        let mut stats = MetricStats::default();
        let m = crate::py_metrics::FunctionMetrics { statements: 3, arguments: 2, ..Default::default() };
        push_py_fn_metrics(&mut stats, &m);
        assert_eq!(stats.statements_per_function, vec![3]);
        assert_eq!(stats.arguments_per_function, vec![2]);
    }

    #[test]
    fn test_collection() {
        let mut stats = MetricStats::default();
        let mut tmp_rs = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(tmp_rs, "fn foo() {{ let x = 1; }}").unwrap();
        let parsed_rs = parse_rust_file(tmp_rs.path()).unwrap();
        assert!(!MetricStats::collect_rust(&[&parsed_rs]).statements_per_file.is_empty());
        let mut tmp_py = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp_py, "def foo():\n    x = 1").unwrap();
        let parsed_py = parse_file(&mut create_parser().unwrap(), tmp_py.path()).unwrap();
        let mut stats2 = MetricStats::default();
        collect_from_node(parsed_py.tree.root_node(), &parsed_py.source, &mut stats2, false);
        assert!(!stats2.statements_per_function.is_empty());
        let m = crate::rust_counts::RustFunctionMetrics { statements: 5, arguments: 2, max_indentation: 1, nested_function_depth: 0, returns: 1, branches: 0, local_variables: 2, bool_parameters: 0, attributes: 0, calls: 3 };
        push_rust_fn_metrics(&mut stats, &m);
        let ast: syn::File = syn::parse_str("fn bar() { let y = 2; }").unwrap();
        collect_rust_from_items(&ast.items, &mut stats);
    }

    #[test]
    fn test_graph_metrics() {
        let mut stats = MetricStats::default();
        let mut graph = crate::graph::DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        stats.collect_graph_metrics(&graph);
        assert!(!stats.fan_in.is_empty());
        assert!(!stats.fan_out.is_empty());
        assert!(!stats.transitive_dependencies.is_empty());
        assert!(!stats.dependency_depth.is_empty());
        assert!(stats.max_depth() > 0);
    }
}
