use super::collect_py::{StatsVisitor, push_py_fn_metrics};
use super::collect_rust::{collect_rust_from_items, push_rust_fn_metrics};
use super::definitions::METRICS;
use super::format::config_key_for;
use super::metric_stats::MetricStats;
use super::percentile::PercentileSummary;
use super::percentile::percentile;
use super::summaries::{compute_summaries, metric_values};
use crate::py_metrics::walk_py_ast;

use crate::parsing::{create_parser, parse_file};
use crate::rust_parsing::parse_rust_file;
use std::io::Write;

#[test]
fn test_stats_helpers() {
    assert_eq!(percentile(&[], 50.0), 0);
    assert_eq!(percentile(&[42], 50.0), 42);
    let s = PercentileSummary::from_values("test_id", &[]);
    assert_eq!(s.count, 0);
    let vals: Vec<usize> = (1..=100).collect();
    assert_eq!(PercentileSummary::from_values("test_id", &vals).max, 100);
    let mut a = MetricStats::default();
    a.statements_per_function.push(5);
    let mut b = MetricStats::default();
    b.statements_per_function.push(10);
    a.merge(b);
    assert_eq!(a.statements_per_function.len(), 2);
    let s2 = MetricStats {
        statements_per_function: vec![1, 2, 3],
        ..Default::default()
    };
    assert!(!compute_summaries(&s2).is_empty());
    assert_eq!(
        config_key_for("statements_per_function"),
        Some("statements_per_function")
    );
    assert_eq!(
        config_key_for("boolean_parameters"),
        Some("boolean_parameters")
    );
    assert!(
        super::format_stats_table(&[PercentileSummary {
            metric_id: "test_id",
            count: 10,
            p50: 5,
            p90: 8,
            p95: 9,
            p99: 10,
            max: 12
        }])
        .contains("test_id")
    );
}

#[test]
fn test_metric_registry() {
    assert!(super::get_metric_def("statements_per_function").is_some());
    assert!(super::get_metric_def("fan_in").is_some());
    assert!(super::get_metric_def("nonexistent").is_none());
    assert_eq!(
        super::get_metric_def("statements_per_function")
            .unwrap()
            .scope,
        super::MetricScope::Function
    );
    assert_eq!(
        super::get_metric_def("fan_in").unwrap().scope,
        super::MetricScope::Module
    );
    assert!(METRICS.len() > 20);
    let def = super::MetricDef {
        metric_id: "test",
        scope: super::MetricScope::Function,
    };
    assert_eq!(def.metric_id, "test");
}

#[test]
fn test_push_py_fn_metrics() {
    let mut stats = MetricStats::default();
    let m = crate::py_metrics::FunctionMetrics {
        statements: 3,
        arguments: 2,
        has_error: false,
        ..Default::default()
    };
    push_py_fn_metrics(&mut stats, &m);
    assert_eq!(stats.statements_per_function, vec![3]);
    assert_eq!(stats.arguments_positional, vec![0]);
}

#[test]
fn test_collection_rust_and_py_parsing() {
    let mut tmp_rs = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp_rs, "fn foo() {{ let x = 1; }}").unwrap();
    let parsed_rs = parse_rust_file(tmp_rs.path()).unwrap();
    assert!(
        !MetricStats::collect_rust(&[&parsed_rs])
            .statements_per_file
            .is_empty()
    );
    let mut tmp_py = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp_py, "def foo():\n    x = 1").unwrap();
    let parsed_py = parse_file(&mut create_parser().unwrap(), tmp_py.path()).unwrap();
    let mut stats2 = MetricStats::default();
    let mut visitor = StatsVisitor { stats: &mut stats2 };
    walk_py_ast(
        parsed_py.tree.root_node(),
        &parsed_py.source,
        &mut |a| visitor.process(a),
        false,
    );
    assert!(!stats2.statements_per_function.is_empty());
}

#[test]
fn test_collection_rust_push_and_items() {
    let mut stats = MetricStats::default();
    let m = crate::rust_counts::RustFunctionMetrics {
        statements: 5,
        arguments: 2,
        max_indentation: 1,
        nested_function_depth: 0,
        returns: 1,
        branches: 0,
        local_variables: 2,
        bool_parameters: 0,
        attributes: 0,
        calls: 3,
    };
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
    graph
        .paths
        .insert("a".into(), std::path::PathBuf::from("a.py"));
    graph
        .paths
        .insert("b".into(), std::path::PathBuf::from("b.py"));
    graph
        .paths
        .insert("c".into(), std::path::PathBuf::from("c.py"));
    stats.collect_graph_metrics(&graph);
    assert!(!stats.fan_in.is_empty());
    assert!(!stats.fan_out.is_empty());
    assert!(!stats.indirect_dependencies.is_empty());
    assert!(!stats.dependency_depth.is_empty());
    assert!(stats.max_depth() > 0);
}

#[test]
fn test_graph_metrics_exclude_external_nodes_from_distributions() {
    let mut stats = MetricStats::default();
    let mut graph = crate::graph::DependencyGraph::new();

    graph.get_or_create_node("a");
    graph
        .paths
        .insert("a".into(), std::path::PathBuf::from("a.py"));
    graph.add_dependency("a", "os");

    stats.collect_graph_metrics(&graph);
    assert_eq!(stats.fan_out.len(), 1);
    assert_eq!(stats.fan_out[0], 1);
}

#[test]
fn test_cycle_size_is_per_module_distribution() {
    let mut stats = MetricStats::default();
    let mut graph = crate::graph::DependencyGraph::new();

    graph.add_dependency("a", "b");
    graph.add_dependency("b", "a");
    graph.get_or_create_node("c");
    graph
        .paths
        .insert("a".into(), std::path::PathBuf::from("a.py"));
    graph
        .paths
        .insert("b".into(), std::path::PathBuf::from("b.py"));
    graph
        .paths
        .insert("c".into(), std::path::PathBuf::from("c.py"));

    stats.collect_graph_metrics(&graph);

    assert_eq!(stats.cycle_size.len(), graph.paths.len());

    let mut got = stats.cycle_size.clone();
    got.sort_unstable();
    assert_eq!(got, vec![0, 2, 2]);
}

#[test]
fn test_metric_values() {
    let mut stats = MetricStats::default();
    stats.statements_per_function.push(10);
    stats.arguments_positional.push(3);
    stats.fan_in.push(2);
    assert_eq!(
        metric_values(&stats, "statements_per_function"),
        Some(&[10][..])
    );
    assert_eq!(metric_values(&stats, "positional_args"), Some(&[3][..]));
    assert_eq!(metric_values(&stats, "fan_in"), Some(&[2][..]));
    assert_eq!(metric_values(&stats, "unknown_metric"), None);
}

#[test]
fn config_key_for_maps_all_metric_families() {
    let cases = [
        ("positional_args", Some("arguments_positional")),
        ("keyword_only_args", Some("arguments_keyword_only")),
        ("branches_per_function", Some("branches_per_function")),
        (
            "local_variables_per_function",
            Some("local_variables_per_function"),
        ),
        ("statements_per_try_block", Some("statements_per_try_block")),
        ("annotations_per_function", Some("annotations_per_function")),
        ("calls_per_function", Some("calls_per_function")),
        ("methods_per_class", Some("methods_per_class")),
        ("lines_per_file", Some("lines_per_file")),
        ("functions_per_file", Some("functions_per_file")),
        ("interface_types_per_file", Some("interface_types_per_file")),
        ("concrete_types_per_file", Some("concrete_types_per_file")),
        ("imported_names_per_file", Some("imported_names_per_file")),
        ("fan_in", Some("fan_in")),
        ("fan_out", Some("fan_out")),
        ("cycle_size", Some("cycle_size")),
        ("indirect_dependencies", Some("indirect_dependencies")),
        ("dependency_depth", Some("dependency_depth")),
        ("unknown_metric", None),
    ];

    for (metric, expected) in cases {
        assert_eq!(config_key_for(metric), expected, "metric={metric}");
    }
}

#[test]
fn format_stats_table_skips_empty_summaries_but_keeps_header() {
    let table = super::format_stats_table(&[PercentileSummary {
        metric_id: "empty_metric",
        count: 0,
        p50: 0,
        p90: 0,
        p95: 0,
        p99: 0,
        max: 0,
    }]);

    assert!(table.contains("metric_id"));
    assert!(!table.contains("empty_metric"));
}
