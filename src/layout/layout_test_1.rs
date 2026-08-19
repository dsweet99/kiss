use super::*;

#[test]
fn test_merge_graph_adds_prefixed_nodes() {
    let mut source = DependencyGraph::new();
    source.add_dependency("a", "b");

    let mut target = DependencyGraph::new();
    merge_graph(&mut target, &source, "py");

    assert!(target.nodes.contains_key("py:a"));
    assert!(target.nodes.contains_key("py:b"));
}

#[test]
fn test_merge_graph_adds_prefixed_edges() {
    let mut source = DependencyGraph::new();
    source.add_dependency("a", "b");

    let mut target = DependencyGraph::new();
    merge_graph(&mut target, &source, "py");

    assert!(target.imports("py:a", "py:b"));
}

#[test]
fn test_clone_graph_without_edges_removes_specified() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    graph.add_dependency("b", "c");
    assert!(graph.imports("a", "b"));
    assert!(graph.imports("b", "c"));

    let edges_to_skip: std::collections::HashSet<_> = vec![("a".to_string(), "b".to_string())]
        .into_iter()
        .collect();
    let new_graph = clone_graph_without_edges(&graph, &edges_to_skip);

    assert!(!new_graph.imports("a", "b"));
    assert!(new_graph.imports("b", "c"));
}

#[test]
fn test_clone_graph_without_edges_handles_empty_skip() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");

    let edges_to_skip = std::collections::HashSet::new();
    let new_graph = clone_graph_without_edges(&graph, &edges_to_skip);

    assert!(new_graph.imports("a", "b"));
}

#[test]
fn test_compute_what_if_shows_improvement() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    graph.add_dependency("b", "a");

    let cycle_analysis = analyze_cycles(&graph);
    let what_if = compute_what_if(&graph, &cycle_analysis);

    assert_eq!(what_if.remaining_cycles, 0);
    assert!(what_if.improvement_summary.contains("clean"));
}

#[test]
fn test_compute_what_if_overlapping_cycles() {



    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    graph.add_dependency("b", "c");
    graph.add_dependency("c", "a");
    graph.add_dependency("c", "d");
    graph.add_dependency("d", "b");

    let cycle_analysis = analyze_cycles(&graph);

    assert!(
        cycle_analysis.cycle_count() >= 1,
        "Expected at least one cycle, got {}",
        cycle_analysis.cycle_count()
    );

    let what_if = compute_what_if(&graph, &cycle_analysis);

    assert!(
        what_if.remaining_cycles <= cycle_analysis.cycle_count(),
        "Breaking edges should not increase cycles"
    );
}

#[test]
fn test_build_py_graph_empty() {
    let graph = crate::analyze::build_py_graph_from_files(&[]).unwrap();
    assert!(graph.nodes.is_empty());
}

#[test]
fn test_build_rs_graph_empty() {
    let graph = crate::analyze::build_rs_graph_from_files(&[]);
    assert!(graph.nodes.is_empty());
}

#[test]
fn test_count_cross_directory_deps() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("mod_a", "mod_b");
    graph.add_dependency("mod_b", "mod_c");
    graph.add_dependency("mod_c", "mod_d");


    graph
        .paths
        .insert("mod_a".to_string(), PathBuf::from("src/mod_a.py"));
    graph
        .paths
        .insert("mod_b".to_string(), PathBuf::from("src/mod_b.py"));

    graph
        .paths
        .insert("mod_c".to_string(), PathBuf::from("utils/mod_c.py"));
    graph
        .paths
        .insert("mod_d".to_string(), PathBuf::from("lib/mod_d.py"));

    let count = count_cross_directory_deps(&graph);



    assert_eq!(count, 2);
}

#[test]
fn test_count_cross_directory_deps_all_same_dir() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    graph.add_dependency("b", "c");

    graph
        .paths
        .insert("a".to_string(), PathBuf::from("src/a.py"));
    graph
        .paths
        .insert("b".to_string(), PathBuf::from("src/b.py"));
    graph
        .paths
        .insert("c".to_string(), PathBuf::from("src/c.py"));

    assert_eq!(count_cross_directory_deps(&graph), 0);
}

#[test]
fn test_count_cross_directory_deps_missing_path() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");

    graph
        .paths
        .insert("a".to_string(), PathBuf::from("src/a.py"));


    assert_eq!(count_cross_directory_deps(&graph), 0);
}

#[test]
fn test_count_layering_violations_dag_no_violations() {



    let mut graph = DependencyGraph::new();
    graph.add_dependency("app", "domain");
    graph.add_dependency("domain", "utils");

    let layer_info = compute_layers(&graph);
    assert_eq!(count_layering_violations(&graph, &layer_info), 0);
}

#[test]
fn test_count_layering_violations_with_cycle() {


    let mut graph = DependencyGraph::new();
    graph.add_dependency("app", "utils");
    graph.add_dependency("utils", "app");

    let layer_info = compute_layers(&graph);

    assert_eq!(count_layering_violations(&graph, &layer_info), 0);
}
