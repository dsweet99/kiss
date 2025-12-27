
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
    pub branches_per_function: Vec<usize>,
    pub local_variables_per_function: Vec<usize>,
    pub methods_per_class: Vec<usize>,
    pub statements_per_file: Vec<usize>,
    pub classes_per_file: Vec<usize>,
    pub imported_names_per_file: Vec<usize>,
    pub fan_in: Vec<usize>,
    pub fan_out: Vec<usize>,
    pub dependency_depth: Vec<usize>,
}

impl MetricStats {
    pub fn collect(parsed_files: &[&ParsedFile]) -> Self {
        let mut stats = Self::default();
        for parsed in parsed_files {
            let fm = compute_file_metrics(parsed);
            stats.statements_per_file.push(fm.statements);
            stats.classes_per_file.push(fm.classes);
            stats.imported_names_per_file.push(fm.imports);
            collect_from_node(parsed.tree.root_node(), &parsed.source, &mut stats, false);
        }
        stats
    }

    pub fn merge(&mut self, other: Self) {
        self.statements_per_function.extend(other.statements_per_function);
        self.arguments_per_function.extend(other.arguments_per_function);
        self.arguments_positional.extend(other.arguments_positional);
        self.arguments_keyword_only.extend(other.arguments_keyword_only);
        self.max_indentation.extend(other.max_indentation);
        self.nested_function_depth.extend(other.nested_function_depth);
        self.returns_per_function.extend(other.returns_per_function);
        self.branches_per_function.extend(other.branches_per_function);
        self.local_variables_per_function.extend(other.local_variables_per_function);
        self.methods_per_class.extend(other.methods_per_class);
        self.statements_per_file.extend(other.statements_per_file);
        self.classes_per_file.extend(other.classes_per_file);
        self.imported_names_per_file.extend(other.imported_names_per_file);
        self.fan_in.extend(other.fan_in);
        self.fan_out.extend(other.fan_out);
        self.dependency_depth.extend(other.dependency_depth);
    }

    pub fn collect_graph_metrics(&mut self, graph: &DependencyGraph) {
        for name in graph.nodes.keys() {
            let m = graph.module_metrics(name);
            self.fan_in.push(m.fan_in);
            self.fan_out.push(m.fan_out);
            self.dependency_depth.push(m.dependency_depth);
        }
    }

    pub fn max_depth(&self) -> usize {
        self.dependency_depth.iter().copied().max().unwrap_or(0)
    }

    pub fn collect_rust(parsed_files: &[&ParsedRustFile]) -> Self {
        let mut stats = Self::default();
        for parsed in parsed_files {
            let fm = compute_rust_file_metrics(parsed);
            stats.statements_per_file.push(fm.statements);
            stats.classes_per_file.push(fm.types);
            stats.imported_names_per_file.push(fm.imports);
            collect_rust_from_items(&parsed.ast.items, &mut stats);
        }
        stats
    }
}

#[allow(clippy::only_used_in_recursion)]
fn collect_from_node(node: Node, source: &str, stats: &mut MetricStats, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let m = compute_function_metrics(node, source);
            stats.statements_per_function.push(m.statements);
            stats.arguments_per_function.push(m.arguments);
            stats.arguments_positional.push(m.arguments_positional);
            stats.arguments_keyword_only.push(m.arguments_keyword_only);
            stats.max_indentation.push(m.max_indentation);
            stats.nested_function_depth.push(m.nested_function_depth);
            stats.returns_per_function.push(m.returns);
            stats.branches_per_function.push(m.branches);
            stats.local_variables_per_function.push(m.local_variables);
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

#[derive(Debug)]
pub struct PercentileSummary {
    pub name: &'static str, pub count: usize, pub p50: usize, pub p90: usize, pub p95: usize, pub p99: usize, pub max: usize,
}

impl PercentileSummary {
    pub fn from_values(name: &'static str, values: &[usize]) -> Self {
        if values.is_empty() { return Self { name, count: 0, p50: 0, p90: 0, p95: 0, p99: 0, max: 0 } }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        Self { name, count: sorted.len(), p50: percentile(&sorted, 50.0), p90: percentile(&sorted, 90.0), 
               p95: percentile(&sorted, 95.0), p99: percentile(&sorted, 99.0), max: *sorted.last().unwrap_or(&0) }
    }
}

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
        PercentileSummary::from_values("Methods per class", &stats.methods_per_class),
        PercentileSummary::from_values("Statements per file", &stats.statements_per_file),
        PercentileSummary::from_values("Classes per file", &stats.classes_per_file),
        PercentileSummary::from_values("Imported names per file", &stats.imported_names_per_file),
        PercentileSummary::from_values("Fan-in (per module)", &stats.fan_in),
        PercentileSummary::from_values("Fan-out (per module)", &stats.fan_out),
        PercentileSummary::from_values("Dependency depth (per module)", &stats.dependency_depth),
    ]
}

pub fn format_stats_table(summaries: &[PercentileSummary]) -> String {
    use std::fmt::Write;
    let mut out = format!("{:<32} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n", "Metric", "Count", "50%", "90%", "95%", "99%", "Max");
    out.push_str(&"-".repeat(74));
    out.push('\n');
    for s in summaries.iter().filter(|s| s.count > 0) {
        let _ = writeln!(out, "{:<32} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}", s.name, s.count, s.p50, s.p90, s.p95, s.p99, s.max);
    }
    out
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
        "Methods per class" => "methods_per_class",
        "Statements per file" => "statements_per_file",
        "Classes per file" => "classes_per_file",
        "Imported names per file" => "imported_names_per_file",
        _ => return None,
    })
}

pub fn generate_config_toml(summaries: &[PercentileSummary]) -> String {
    use std::fmt::Write;
    let mut out = String::from("# Generated by kiss mimic\n# Thresholds based on 99th percentile\n\n[thresholds]\n");
    for s in summaries { if let Some(k) = config_key_for(s.name) { let _ = writeln!(out, "{k} = {}", s.p99); } }
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
        let s = PercentileSummary::from_values("test", &[]);
        assert_eq!(s.count, 0);
        let vals: Vec<usize> = (1..=100).collect();
        assert_eq!(PercentileSummary::from_values("test", &vals).max, 100);
        let mut a = MetricStats::default();
        a.statements_per_function.push(5);
        let mut b = MetricStats::default();
        b.statements_per_function.push(10);
        a.merge(b);
        assert_eq!(a.statements_per_function.len(), 2);
        let s2 = MetricStats { statements_per_function: vec![1, 2, 3], ..Default::default() };
        assert!(!compute_summaries(&s2).is_empty());
        let toml = generate_config_toml(&[PercentileSummary { name: "Statements per function", count: 10, p50: 5, p90: 9, p95: 10, p99: 15, max: 20 }]);
        assert!(toml.contains("statements_per_function = 15"));
        assert_eq!(config_key_for("Statements per function"), Some("statements_per_function"));
        assert!(format_stats_table(&[PercentileSummary { name: "Test", count: 10, p50: 5, p90: 8, p95: 9, p99: 10, max: 12 }]).contains("Test"));
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
        let m = crate::rust_counts::RustFunctionMetrics { statements: 5, arguments: 2, max_indentation: 1, nested_function_depth: 0, returns: 1, branches: 0, local_variables: 2, bool_parameters: 0, attributes: 0 };
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
        assert!(!stats.dependency_depth.is_empty());
        assert!(stats.max_depth() > 0);
    }
}
