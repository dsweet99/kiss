use crate::graph::DependencyGraph;
use petgraph::Direction;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
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
            .flat_map(|(layer_num, modules)| modules.iter().map(move |m| (m.clone(), layer_num)))
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
#[path = "layout_layers_test.rs"]
mod tests;
