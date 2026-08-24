use super::*;

fn build_graph(edges: &[(&str, &str)]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    for (from, to) in edges {
        graph.add_dependency(from, to);
    }
    graph
}

#[test]
fn test_empty_graph() {
    let graph = DependencyGraph::new();
    let info = compute_layers(&graph);
    assert_eq!(info.num_layers(), 0);
    assert!(info.layers.is_empty());
}

#[test]
fn test_single_node_no_edges() {
    let mut graph = DependencyGraph::new();
    graph.get_or_create_node("a");
    let info = compute_layers(&graph);
    assert_eq!(info.num_layers(), 1);
    assert_eq!(info.layers[0], vec!["a"]);
    assert_eq!(info.layer_for("a"), Some(0));
}

#[test]
fn test_linear_chain() {
    let graph = build_graph(&[("c", "b"), ("b", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 3);
    assert_eq!(info.layer_for("a"), Some(0));
    assert_eq!(info.layer_for("b"), Some(1));
    assert_eq!(info.layer_for("c"), Some(2));
}

#[test]
fn test_diamond_dependency() {
    let graph = build_graph(&[("d", "b"), ("d", "c"), ("b", "a"), ("c", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 3);
    assert_eq!(info.layer_for("a"), Some(0));
    assert_eq!(info.layer_for("b"), Some(1));
    assert_eq!(info.layer_for("c"), Some(1));
    assert_eq!(info.layer_for("d"), Some(2));
}

#[test]
fn test_two_node_cycle() {
    let graph = build_graph(&[("a", "b"), ("b", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 1);
    let layer_a = info.layer_for("a").unwrap();
    let layer_b = info.layer_for("b").unwrap();
    assert_eq!(layer_a, layer_b, "Cycle members should share a layer");
}

#[test]
fn test_cycle_with_dependency() {
    let graph = build_graph(&[("a", "b"), ("b", "a"), ("c", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 2);
    assert_eq!(info.layer_for("a"), Some(0));
    assert_eq!(info.layer_for("b"), Some(0));
    assert_eq!(info.layer_for("c"), Some(1));
}

#[test]
fn test_cycle_depends_on_foundation() {
    let graph = build_graph(&[("a", "b"), ("b", "a"), ("b", "utils")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 2);
    assert_eq!(info.layer_for("utils"), Some(0));
    assert_eq!(info.layer_for("a"), Some(1));
    assert_eq!(info.layer_for("b"), Some(1));
}

#[test]
fn test_three_node_cycle() {
    let graph = build_graph(&[("a", "b"), ("b", "c"), ("c", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 1);
    let layers: Vec<_> = ["a", "b", "c"].iter().map(|m| info.layer_for(m)).collect();
    assert!(
        layers.iter().all(|l| *l == Some(0)),
        "All cycle members should be at layer 0"
    );
}

#[test]
fn test_multiple_foundations() {
    let graph = build_graph(&[("c", "a"), ("c", "b")]);
    let info = compute_layers(&graph);

    assert_eq!(info.num_layers(), 2);
    assert_eq!(info.layer_for("a"), Some(0));
    assert_eq!(info.layer_for("b"), Some(0));
    assert_eq!(info.layer_for("c"), Some(1));
}

#[test]
fn test_all_assignments() {
    let graph = build_graph(&[("b", "a")]);
    let info = compute_layers(&graph);
    let assignments = info.all_assignments();

    assert_eq!(assignments.len(), 2);
    assert!(assignments.contains(&("a".to_string(), 0)));
    assert!(assignments.contains(&("b".to_string(), 1)));
}

#[test]
fn test_layer_for_unknown_module() {
    let graph = build_graph(&[("a", "b")]);
    let info = compute_layers(&graph);
    assert_eq!(info.layer_for("unknown"), None);
}

#[test]
fn test_multiple_isolated_nodes() {
    let mut graph = DependencyGraph::new();
    graph.get_or_create_node("a");
    graph.get_or_create_node("b");
    graph.get_or_create_node("c");

    let info = compute_layers(&graph);
    assert_eq!(
        info.num_layers(),
        1,
        "All isolated nodes should be at layer 0"
    );
    assert_eq!(info.layer_for("a"), Some(0));
    assert_eq!(info.layer_for("b"), Some(0));
    assert_eq!(info.layer_for("c"), Some(0));
}

#[test]
fn test_complex_graph_with_multiple_sccs() {
    let graph = build_graph(&[
        ("core", "utils"),
        ("config", "utils"),
        ("core", "config"),
        ("config", "core"),
        ("api", "core"),
    ]);
    let info = compute_layers(&graph);

    assert_eq!(info.layer_for("utils"), Some(0));
    let core_layer = info.layer_for("core").unwrap();
    let config_layer = info.layer_for("config").unwrap();
    assert_eq!(core_layer, config_layer, "core and config in same SCC");
    assert!(core_layer > 0, "SCC should be above utils");
    let api_layer = info.layer_for("api").unwrap();
    assert!(api_layer > core_layer, "api should be above the SCC");
}

#[test]
fn test_build_condensation_single_node() {
    let mut graph = DependencyGraph::new();
    graph.get_or_create_node("only_node");
    let info = compute_layers(&graph);

    assert_eq!(
        info.num_layers(),
        1,
        "Single node condensation should yield one layer"
    );
    assert_eq!(info.layers[0].len(), 1, "Exactly one node in condensation");
    assert_eq!(info.layer_for("only_node"), Some(0));
}

#[test]
fn test_build_condensation_preserves_edges() {
    let graph = build_graph(&[("a", "b"), ("b", "c")]);
    let info = compute_layers(&graph);

    assert_eq!(
        info.num_layers(),
        3,
        "Three SCCs with inter-edges should have 3 layers"
    );
    assert_eq!(info.layer_for("c"), Some(0), "c has no outgoing edges");
    assert_eq!(info.layer_for("b"), Some(1), "b depends on c");
    assert_eq!(info.layer_for("a"), Some(2), "a depends on b");
}

#[test]
fn test_build_condensation_removes_intra_scc_edges() {
    let graph = build_graph(&[("a", "b"), ("b", "c"), ("c", "a"), ("d", "a")]);
    let info = compute_layers(&graph);

    assert_eq!(
        info.num_layers(),
        2,
        "One SCC + one external node = 2 layers"
    );
    assert_eq!(info.layer_for("a"), Some(0), "a is in the SCC at layer 0");
    assert_eq!(info.layer_for("b"), Some(0), "b is in the SCC at layer 0");
    assert_eq!(info.layer_for("c"), Some(0), "c is in the SCC at layer 0");
    assert_eq!(info.layer_for("d"), Some(1), "d depends on SCC, so layer 1");
}

#[test]
fn test_compute_layer_for_node_cached() {
    let graph = build_graph(&[
        ("top", "mid1"),
        ("top", "mid2"),
        ("mid1", "base"),
        ("mid2", "base"),
    ]);
    let info = compute_layers(&graph);

    assert_eq!(info.layer_for("base"), Some(0), "base is foundation");
    assert_eq!(info.layer_for("mid1"), Some(1), "mid1 depends on base");
    assert_eq!(info.layer_for("mid2"), Some(1), "mid2 depends on base");
    assert_eq!(
        info.layer_for("top"),
        Some(2),
        "top depends on mid1 and mid2"
    );

    let info2 = compute_layers(&graph);
    assert_eq!(
        info.layers, info2.layers,
        "Repeated computation should be identical"
    );
}

#[test]
fn test_deep_linear_chain_no_stack_overflow() {
    let depth = 1000;
    let mut graph = DependencyGraph::new();

    for i in (1..depth).rev() {
        let from = format!("n{i}");
        let to = format!("n{}", i - 1);
        graph.add_dependency(&from, &to);
    }

    let info = compute_layers(&graph);

    assert_eq!(
        info.num_layers(),
        depth,
        "Expected {depth} layers for linear chain"
    );
    assert_eq!(info.layer_for("n0"), Some(0), "n0 is foundation");
    assert_eq!(
        info.layer_for(&format!("n{}", depth - 1)),
        Some(depth - 1),
        "Top node should be at layer {}",
        depth - 1
    );
}
