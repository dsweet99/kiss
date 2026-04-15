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
                "Edge '{}' -> '{}' selected (alphabetically first). Consider min-cut analysis for optimal break point.",
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
#[path = "layout_cycles_test.rs"]
mod tests;
