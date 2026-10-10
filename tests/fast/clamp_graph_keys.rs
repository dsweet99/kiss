use kiss::{Config, DependencyGraph, analyze_graph, graph_key_maxima};
use std::path::PathBuf;

fn sample_graph() -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("cycle_a", "cycle_b");
    graph.add_dependency("cycle_b", "cycle_a");
    graph.paths.insert("cycle_a".into(), PathBuf::from("a.py"));
    graph.paths.insert("cycle_b".into(), PathBuf::from("b.py"));
    graph.add_dependency("ghost", "cycle_a");
    graph.add_dependency("ghost", "tail1");
    graph.add_dependency("tail1", "tail2");
    graph.add_dependency("tail2", "tail3");
    graph.add_dependency("tail3", "tail4");
    graph.add_dependency("lib", "mid");
    graph.add_dependency("mid", "deep1");
    graph.add_dependency("deep1", "deep2");
    graph.add_dependency("deep2", "deep3");
    graph
        .paths
        .insert("lib".into(), PathBuf::from("src/lib.rs"));
    graph
        .paths
        .insert("mid".into(), PathBuf::from("src/mid.rs"));
    graph
        .paths
        .insert("deep1".into(), PathBuf::from("src/deep1.rs"));
    graph
        .paths
        .insert("deep2".into(), PathBuf::from("src/deep2.rs"));
    graph
        .paths
        .insert("deep3".into(), PathBuf::from("src/deep3.rs"));
    graph
}

fn assert_cycle_size_matches(graph: &DependencyGraph) {
    let max = graph_key_maxima(graph);
    let cycle_size = graph
        .find_cycles()
        .cycles
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    assert_eq!(max.cycle_size, cycle_size);
}

fn assert_pathless_nodes_are_not_maxima(graph: &DependencyGraph) {
    let max = graph_key_maxima(graph);
    let lib_indirect = graph.module_metrics("lib").indirect_dependencies;
    let mid_indirect = graph.module_metrics("mid").indirect_dependencies;
    let ghost_indirect = graph.module_metrics("ghost").indirect_dependencies;
    assert!(lib_indirect > mid_indirect);
    assert!(ghost_indirect > lib_indirect);
    assert_eq!(max.indirect_dependencies, mid_indirect);
    let path_depth = graph.module_metrics("lib").dependency_depth.max(
        graph
            .module_metrics("mid")
            .dependency_depth
            .max(graph.module_metrics("cycle_a").dependency_depth),
    );
    assert_eq!(max.dependency_depth, path_depth);
}

fn assert_analyze_skips_pathless_and_aggregators(graph: &DependencyGraph) {
    let mut config = Config::python_defaults();
    config.cycle_size = 0;
    config.indirect_dependencies = 0;
    config.dependency_depth = 0;
    let viols = analyze_graph(graph, &config);
    assert!(
        !viols.iter().any(|v| v.unit_name == "ghost"),
        "pathless nodes must not get their own violation"
    );
    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "indirect_dependencies" && v.unit_name == "lib"),
        "crate-root aggregators must be skipped for indirect_dependencies"
    );
}

#[test]
fn graph_keys_match_analyze_graph() {
    let graph = sample_graph();
    assert_cycle_size_matches(&graph);
    assert_pathless_nodes_are_not_maxima(&graph);
    assert_analyze_skips_pathless_and_aggregators(&graph);
}
