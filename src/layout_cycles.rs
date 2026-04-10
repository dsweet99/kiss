//! Cycle detection and breaking suggestions for the `kiss layout` command.
//!
//! Uses Tarjan's SCC algorithm to find all strongly connected components (cycles),
//! then suggests which edge to break based on heuristics.

use crate::graph::DependencyGraph;
use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;

/// Information about a single cycle (strongly connected component) with a suggestion
/// for which edge to break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleBreakSuggestion {
    /// Module names in the cycle
    pub modules: Vec<String>,
    /// Suggested edge to break: (`from_module`, `to_module`)
    pub suggested_break: (String, String),
    /// Human-readable reason for the suggestion
    pub reason: String,
}

/// Result of cycle analysis for layout purposes.
#[derive(Debug, Clone, Default)]
pub struct LayoutCycleAnalysis {
    /// All cycles found with break suggestions
    pub cycles: Vec<CycleBreakSuggestion>,
}

impl LayoutCycleAnalysis {
    /// Returns true if there are no cycles.
    #[must_use]
    pub const fn is_acyclic(&self) -> bool {
        self.cycles.is_empty()
    }

    /// Returns the number of cycles found.
    #[must_use]
    pub const fn cycle_count(&self) -> usize {
        self.cycles.len()
    }
}

/// Find all strongly connected components larger than size 1 (true cycles).
///
/// Returns SCCs as vectors of `NodeIndex`, filtered to only include actual cycles.
fn find_nontrivial_sccs(graph: &DependencyGraph) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(&graph.graph)
        .into_iter()
        .filter(|scc| is_nontrivial_cycle(graph, scc))
        .collect()
}

/// Returns true if the SCC represents a true cycle:
/// - Size > 1: multiple nodes forming a cycle
/// - Size == 1: only if the node has a self-loop
fn is_nontrivial_cycle(graph: &DependencyGraph, scc: &[NodeIndex]) -> bool {
    match scc.len() {
        0 => false,
        1 => graph.graph.contains_edge(scc[0], scc[0]),
        _ => true,
    }
}

/// Find a deterministic edge in an SCC to suggest breaking.
///
/// Since edges have no weight data, this picks the edge with the
/// alphabetically first (source, target) pair. This ensures consistent,
/// reproducible suggestions across runs.
///
/// Returns `(from_module, to_module)` or `None` if no edge found.
fn find_deterministic_break_edge(graph: &DependencyGraph, scc: &[NodeIndex]) -> Option<(String, String)> {
    use std::collections::HashSet;

    let scc_set: HashSet<NodeIndex> = scc.iter().copied().collect();

    let mut candidate: Option<(String, String)> = None;

    for &node in scc {
        let from_name = graph.graph[node].clone();

        for neighbor in graph
            .graph
            .neighbors_directed(node, petgraph::Direction::Outgoing)
        {
            if !scc_set.contains(&neighbor) {
                continue;
            }

            let to_name = graph.graph[neighbor].clone();

            let should_update = match &candidate {
                None => true,
                Some((curr_from, curr_to)) => {
                    (&from_name, &to_name) < (curr_from, curr_to)
                }
            };

            if should_update {
                candidate = Some((from_name.clone(), to_name));
            }
        }
    }

    candidate
}

/// Analyze cycles in a dependency graph and suggest breaks.
///
/// Returns structured data for each cycle including:
/// - List of modules in the cycle
/// - Suggested edge to break
/// - Reason for the suggestion
#[must_use]
pub fn analyze_cycles(graph: &DependencyGraph) -> LayoutCycleAnalysis {
    let sccs = find_nontrivial_sccs(graph);

    let cycles = sccs
        .into_iter()
        .filter_map(|scc| {
            let modules: Vec<String> = scc.iter().map(|&idx| graph.graph[idx].clone()).collect();

            let suggested_break = find_deterministic_break_edge(graph, &scc)?;

            let reason = format!(
                "Edge '{}' -> '{}' selected (alphabetically first source in cycle)",
                suggested_break.0, suggested_break.1
            );

            Some(CycleBreakSuggestion {
                modules,
                suggested_break,
                reason,
            })
        })
        .collect();

    LayoutCycleAnalysis { cycles }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph_with_cycle() -> DependencyGraph {
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b");
        g.add_dependency("b", "c");
        g.add_dependency("c", "a");
        g
    }

    fn make_graph_no_cycle() -> DependencyGraph {
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b");
        g.add_dependency("b", "c");
        g
    }

    #[test]
    fn test_find_nontrivial_sccs_detects_cycle() {
        let g = make_graph_with_cycle();
        let sccs = find_nontrivial_sccs(&g);
        assert_eq!(sccs.len(), 1, "Expected one cycle SCC");
        assert_eq!(sccs[0].len(), 3, "Expected 3-node cycle");
    }

    #[test]
    fn test_find_nontrivial_sccs_no_cycle() {
        let g = make_graph_no_cycle();
        let sccs = find_nontrivial_sccs(&g);
        assert!(sccs.is_empty(), "Expected no cycles in DAG");
    }

    #[test]
    fn test_find_nontrivial_sccs_self_loop() {
        let mut g = DependencyGraph::new();
        g.get_or_create_node("a");
        let idx = *g.nodes.get("a").unwrap();
        g.graph.add_edge(idx, idx, ());

        let sccs = find_nontrivial_sccs(&g);
        assert_eq!(sccs.len(), 1, "Self-loop should be detected as cycle");
        assert_eq!(sccs[0].len(), 1);
    }

    #[test]
    fn test_is_nontrivial_cycle_empty() {
        let g = DependencyGraph::new();
        assert!(!is_nontrivial_cycle(&g, &[]));
    }

    #[test]
    fn test_find_deterministic_break_edge_picks_alphabetically_first() {
        let mut g = DependencyGraph::new();
        g.add_dependency("zebra", "apple");
        g.add_dependency("apple", "zebra");

        let scc: Vec<NodeIndex> = g.nodes.values().copied().collect();
        let edge = find_deterministic_break_edge(&g, &scc);

        assert_eq!(
            edge,
            Some(("apple".to_string(), "zebra".to_string())),
            "Should pick alphabetically first source"
        );
    }

    #[test]
    fn test_find_deterministic_break_edge_three_node_cycle() {
        let g = make_graph_with_cycle();
        let scc: Vec<NodeIndex> = g.nodes.values().copied().collect();
        let edge = find_deterministic_break_edge(&g, &scc);

        assert!(edge.is_some());
        let (from, to) = edge.unwrap();
        assert_eq!(from, "a", "Alphabetically first source should be 'a'");
        assert_eq!(to, "b");
    }

    #[test]
    fn test_analyze_cycles_returns_suggestions() {
        let g = make_graph_with_cycle();
        let analysis = analyze_cycles(&g);

        assert_eq!(analysis.cycle_count(), 1);
        assert!(!analysis.is_acyclic());

        let cycle = &analysis.cycles[0];
        assert_eq!(cycle.modules.len(), 3);
        assert_eq!(cycle.suggested_break.0, "a");
        assert_eq!(cycle.suggested_break.1, "b");
        assert!(cycle.reason.contains("alphabetically first"));
    }

    #[test]
    fn test_analyze_cycles_no_cycles() {
        let g = make_graph_no_cycle();
        let analysis = analyze_cycles(&g);

        assert!(analysis.is_acyclic());
        assert_eq!(analysis.cycle_count(), 0);
    }

    #[test]
    fn test_analyze_cycles_multiple_cycles() {
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b");
        g.add_dependency("b", "a");
        g.add_dependency("x", "y");
        g.add_dependency("y", "z");
        g.add_dependency("z", "x");

        let analysis = analyze_cycles(&g);
        assert_eq!(analysis.cycle_count(), 2, "Expected two separate cycles");
    }

    #[test]
    fn test_layout_cycle_analysis_default() {
        let analysis = LayoutCycleAnalysis::default();
        assert!(analysis.is_acyclic());
        assert_eq!(analysis.cycle_count(), 0);
    }

    #[test]
    fn test_cycle_break_suggestion_clone_and_eq() {
        let suggestion = CycleBreakSuggestion {
            modules: vec!["a".to_string(), "b".to_string()],
            suggested_break: ("a".to_string(), "b".to_string()),
            reason: "test".to_string(),
        };
        let cloned = suggestion.clone();
        assert_eq!(suggestion, cloned);
    }

    #[test]
    fn test_find_deterministic_break_edge_self_loop_returns_edge() {
        let mut g = DependencyGraph::new();
        g.get_or_create_node("a");
        let idx = *g.nodes.get("a").unwrap();
        g.graph.add_edge(idx, idx, ());

        let scc = vec![idx];
        let edge = find_deterministic_break_edge(&g, &scc);

        assert!(
            edge.is_some(),
            "find_deterministic_break_edge should return the self-loop edge"
        );
        let (from, to) = edge.unwrap();
        assert_eq!(from, "a");
        assert_eq!(to, "a");
    }
}
