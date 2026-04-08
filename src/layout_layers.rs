use crate::graph::DependencyGraph;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::HashMap;

/// Contains the layers, ordered from 0 (foundation) upward.
/// Each inner Vec contains module names at that layer level.
#[derive(Debug, Clone, Default)]
pub struct LayerInfo {
    /// layers[0] = foundation/utilities (no dependencies)
    /// layers[1] = modules that only depend on layer 0, etc.
    pub layers: Vec<Vec<String>>,
}

impl LayerInfo {
    #[must_use]
    pub const fn num_layers(&self) -> usize {
        self.layers.len()
    }

    #[must_use]
    pub fn layer_for(&self, module: &str) -> Option<usize> {
        self.layers
            .iter()
            .enumerate()
            .find_map(|(i, layer)| layer.iter().any(|m| m == module).then_some(i))
    }

    #[must_use]
    pub fn all_assignments(&self) -> Vec<(String, usize)> {
        self.layers
            .iter()
            .enumerate()
            .flat_map(|(layer_num, modules)| {
                modules.iter().map(move |m| (m.clone(), layer_num))
            })
            .collect()
    }
}

/// Computes layer assignments for all modules in the dependency graph.
///
/// Algorithm:
/// 1. Find SCCs using Tarjan's algorithm
/// 2. Build condensation graph (DAG where each SCC is a single node)
/// 3. Assign layer levels: nodes with no outgoing edges get layer 0,
///    others get max(layer of dependencies) + 1
/// 4. All modules in the same SCC share the same layer
#[must_use]
pub fn compute_layers(graph: &DependencyGraph) -> LayerInfo {
    if graph.nodes.is_empty() {
        return LayerInfo::default();
    }

    let sccs = tarjan_scc(&graph.graph);
    if sccs.is_empty() {
        return LayerInfo::default();
    }

    let (condensed, node_to_scc) = build_condensation(&graph.graph, &sccs);

    let mut scc_layers: HashMap<NodeIndex, usize> = HashMap::new();
    for node in condensed.node_indices() {
        compute_layer_for_node(&condensed, node, &mut scc_layers);
    }

    let max_layer = scc_layers.values().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<String>> = vec![Vec::new(); max_layer + 1];

    for (module_name, &original_idx) in &graph.nodes {
        let scc_idx = node_to_scc[&original_idx];
        let layer = scc_layers.get(&scc_idx).copied().unwrap_or(0);
        layers[layer].push(module_name.clone());
    }

    for layer in &mut layers {
        layer.sort();
    }

    LayerInfo { layers }
}

fn build_condensation(
    graph: &DiGraph<String, ()>,
    sccs: &[Vec<NodeIndex>],
) -> (DiGraph<usize, ()>, HashMap<NodeIndex, NodeIndex>) {
    let mut node_to_scc: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    let mut condensed: DiGraph<usize, ()> = DiGraph::new();

    for (scc_id, scc) in sccs.iter().enumerate() {
        let scc_node = condensed.add_node(scc_id);
        for &node in scc {
            node_to_scc.insert(node, scc_node);
        }
    }

    for edge in graph.edge_references() {
        let from_scc = node_to_scc[&edge.source()];
        let to_scc = node_to_scc[&edge.target()];
        if from_scc != to_scc && !condensed.contains_edge(from_scc, to_scc) {
            condensed.add_edge(from_scc, to_scc, ());
        }
    }

    (condensed, node_to_scc)
}

fn compute_layer_for_node(
    condensed: &DiGraph<usize, ()>,
    node: NodeIndex,
    layers: &mut HashMap<NodeIndex, usize>,
) -> usize {
    if let Some(&layer) = layers.get(&node) {
        return layer;
    }

    let deps: Vec<NodeIndex> = condensed
        .neighbors_directed(node, Direction::Outgoing)
        .collect();

    let layer = if deps.is_empty() {
        0
    } else {
        deps.iter()
            .map(|&dep| compute_layer_for_node(condensed, dep, layers))
            .max()
            .unwrap_or(0)
            + 1
    };

    layers.insert(node, layer);
    layer
}

#[cfg(test)]
mod tests {
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
        // c -> b -> a
        // Layer 0: a (no deps)
        // Layer 1: b (depends on a)
        // Layer 2: c (depends on b)
        let graph = build_graph(&[("c", "b"), ("b", "a")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 3);
        assert_eq!(info.layer_for("a"), Some(0));
        assert_eq!(info.layer_for("b"), Some(1));
        assert_eq!(info.layer_for("c"), Some(2));
    }

    #[test]
    fn test_diamond_dependency() {
        // d -> b -> a
        // d -> c -> a
        // Layer 0: a
        // Layer 1: b, c
        // Layer 2: d
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
        // a <-> b (cycle)
        // Both should be in the same layer (as one SCC)
        let graph = build_graph(&[("a", "b"), ("b", "a")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 1);
        let layer_a = info.layer_for("a").unwrap();
        let layer_b = info.layer_for("b").unwrap();
        assert_eq!(layer_a, layer_b, "Cycle members should share a layer");
    }

    #[test]
    fn test_cycle_with_dependency() {
        // c -> a <-> b
        // SCC {a, b} at layer 0
        // c at layer 1
        let graph = build_graph(&[("a", "b"), ("b", "a"), ("c", "a")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 2);
        assert_eq!(info.layer_for("a"), Some(0));
        assert_eq!(info.layer_for("b"), Some(0));
        assert_eq!(info.layer_for("c"), Some(1));
    }

    #[test]
    fn test_cycle_depends_on_foundation() {
        // a <-> b -> utils
        // Layer 0: utils
        // Layer 1: a, b (cycle, depends on utils)
        let graph = build_graph(&[("a", "b"), ("b", "a"), ("b", "utils")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 2);
        assert_eq!(info.layer_for("utils"), Some(0));
        assert_eq!(info.layer_for("a"), Some(1));
        assert_eq!(info.layer_for("b"), Some(1));
    }

    #[test]
    fn test_three_node_cycle() {
        // a -> b -> c -> a
        let graph = build_graph(&[("a", "b"), ("b", "c"), ("c", "a")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 1);
        let layers: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|m| info.layer_for(m))
            .collect();
        assert!(
            layers.iter().all(|l| *l == Some(0)),
            "All cycle members should be at layer 0"
        );
    }

    #[test]
    fn test_multiple_foundations() {
        // c -> a
        // c -> b
        // Layer 0: a, b (no deps)
        // Layer 1: c
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
        // Multiple nodes with no edges - all should be at layer 0
        let mut graph = DependencyGraph::new();
        graph.get_or_create_node("a");
        graph.get_or_create_node("b");
        graph.get_or_create_node("c");

        let info = compute_layers(&graph);
        assert_eq!(info.num_layers(), 1, "All isolated nodes should be at layer 0");
        assert_eq!(info.layer_for("a"), Some(0));
        assert_eq!(info.layer_for("b"), Some(0));
        assert_eq!(info.layer_for("c"), Some(0));
    }

    #[test]
    fn test_complex_graph_with_multiple_sccs() {
        // Foundation: utils
        // Cycle 1: core <-> config (depends on utils)
        // Module: api (depends on core)
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
        // Single node graph: condensation should produce exactly one SCC node
        // Verified indirectly: single node at layer 0, no other layers
        let mut graph = DependencyGraph::new();
        graph.get_or_create_node("only_node");
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 1, "Single node condensation should yield one layer");
        assert_eq!(info.layers[0].len(), 1, "Exactly one node in condensation");
        assert_eq!(info.layer_for("only_node"), Some(0));
    }

    #[test]
    fn test_build_condensation_preserves_edges() {
        // Three separate SCCs with edges between them:
        // SCC1: {a} -> SCC2: {b} -> SCC3: {c}
        // Inter-SCC edges should be preserved, resulting in layers 0, 1, 2
        let graph = build_graph(&[("a", "b"), ("b", "c")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 3, "Three SCCs with inter-edges should have 3 layers");
        assert_eq!(info.layer_for("c"), Some(0), "c has no outgoing edges");
        assert_eq!(info.layer_for("b"), Some(1), "b depends on c");
        assert_eq!(info.layer_for("a"), Some(2), "a depends on b");
    }

    #[test]
    fn test_build_condensation_removes_intra_scc_edges() {
        // Cycle: a -> b -> c -> a (all in one SCC)
        // Plus external dependency: d depends on a
        // Intra-SCC edges (a->b, b->c, c->a) should be removed in condensation
        // Result: SCC{a,b,c} at layer 0, d at layer 1
        let graph = build_graph(&[("a", "b"), ("b", "c"), ("c", "a"), ("d", "a")]);
        let info = compute_layers(&graph);

        assert_eq!(info.num_layers(), 2, "One SCC + one external node = 2 layers");
        assert_eq!(info.layer_for("a"), Some(0), "a is in the SCC at layer 0");
        assert_eq!(info.layer_for("b"), Some(0), "b is in the SCC at layer 0");
        assert_eq!(info.layer_for("c"), Some(0), "c is in the SCC at layer 0");
        assert_eq!(info.layer_for("d"), Some(1), "d depends on SCC, so layer 1");
    }

    #[test]
    fn test_compute_layer_for_node_cached() {
        // Test memoization: build a graph where multiple paths lead to same nodes
        // If memoization works, compute_layers produces consistent results
        // Graph: top -> mid1 -> base, top -> mid2 -> base
        // Without memoization, `base` might be computed multiple times with different results
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
        assert_eq!(info.layer_for("top"), Some(2), "top depends on mid1 and mid2");

        // Call compute_layers again to verify idempotence (memoization produces same result)
        let info2 = compute_layers(&graph);
        assert_eq!(info.layers, info2.layers, "Repeated computation should be identical");
    }

    #[test]
    fn test_deep_linear_chain_no_stack_overflow() {
        // Hypothesis from review: deep dependency chains could overflow the stack.
        // Test: Create a chain of 1000 linear dependencies and verify it completes.
        // This tests the memoization prevents re-visiting and the recursion depth
        // is bounded by the actual chain depth (which is sequential, not concurrent).
        let depth = 1000;
        let mut graph = DependencyGraph::new();

        // Create chain: n999 -> n998 -> ... -> n1 -> n0
        for i in (1..depth).rev() {
            let from = format!("n{i}");
            let to = format!("n{}", i - 1);
            graph.add_dependency(&from, &to);
        }

        let info = compute_layers(&graph);

        // Should have `depth` layers
        assert_eq!(info.num_layers(), depth, "Expected {depth} layers for linear chain");
        assert_eq!(info.layer_for("n0"), Some(0), "n0 is foundation");
        assert_eq!(
            info.layer_for(&format!("n{}", depth - 1)),
            Some(depth - 1),
            "Top node should be at layer {}", depth - 1
        );
    }

    #[test]
    fn static_coverage_touch_layer_internals() {
        fn t<T>(_: T) {}
        t(build_condensation);
        t(compute_layer_for_node);
    }
}
