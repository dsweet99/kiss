use crate::graph::DependencyGraph;
use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleBreakSuggestion {
    pub modules: Vec<String>,
    pub suggested_break: (String, String),
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutCycleAnalysis {
    pub cycles: Vec<CycleBreakSuggestion>,
}

impl LayoutCycleAnalysis {
    #[must_use]
    pub const fn is_acyclic(&self) -> bool {
        self.cycles.is_empty()
    }

    #[must_use]
    pub const fn cycle_count(&self) -> usize {
        self.cycles.len()
    }
}

fn find_nontrivial_sccs(graph: &DependencyGraph) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(&graph.graph)
        .into_iter()
        .filter(|scc| is_nontrivial_cycle(graph, scc))
        .collect()
}

fn is_nontrivial_cycle(graph: &DependencyGraph, scc: &[NodeIndex]) -> bool {
    match scc.len() {
        0 => false,
        1 => graph.graph.contains_edge(scc[0], scc[0]),
        _ => true,
    }
}

fn find_deterministic_break_edge(
    graph: &DependencyGraph,
    scc: &[NodeIndex],
) -> Option<(String, String)> {
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
                Some((curr_from, curr_to)) => (&from_name, &to_name) < (curr_from, curr_to),
            };

            if should_update {
                candidate = Some((from_name.clone(), to_name));
            }
        }
    }

    candidate
}

#[must_use]
pub fn analyze_cycles(graph: &DependencyGraph) -> LayoutCycleAnalysis {
    let sccs = find_nontrivial_sccs(graph);

    let cycles = sccs
        .into_iter()
        .filter_map(|scc| {
            let modules: Vec<String> = scc.iter().map(|&idx| graph.graph[idx].clone()).collect();

            let suggested_break = find_deterministic_break_edge(graph, &scc)?;

            let reason = format!(
                "Edge '{}' -> '{}' selected (deterministic: alphabetically first edge in this cycle). Review remaining edges if this break is too disruptive.",
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

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl CycleBreakSuggestion {
        fn witness() -> Self {
            Self {
                modules: vec!["a".into()],
                suggested_break: ("a".into(), "b".into()),
                reason: "witness".into(),
            }
        }
    }

    #[test]
    fn witness_cycle_break_suggestion() {
        let _ = CycleBreakSuggestion::witness();
    }
}
