use super::*;
use petgraph::graph::NodeIndex;

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
    let sccs = super::find_nontrivial_sccs(&g);
    assert_eq!(sccs.len(), 1, "Expected one cycle SCC");
    assert_eq!(sccs[0].len(), 3, "Expected 3-node cycle");
}

#[test]
fn test_find_nontrivial_sccs_no_cycle() {
    let g = make_graph_no_cycle();
    let sccs = super::find_nontrivial_sccs(&g);
    assert!(sccs.is_empty(), "Expected no cycles in DAG");
}

#[test]
fn test_find_nontrivial_sccs_self_loop() {
    let mut g = DependencyGraph::new();
    g.get_or_create_node("a");
    let idx = *g.nodes.get("a").unwrap();
    g.graph.add_edge(idx, idx, ());

    let sccs = super::find_nontrivial_sccs(&g);
    assert_eq!(sccs.len(), 1, "Self-loop should be detected as cycle");
    assert_eq!(sccs[0].len(), 1);
}

#[test]
fn test_is_nontrivial_cycle_empty() {
    let g = DependencyGraph::new();
    assert!(!super::is_nontrivial_cycle(&g, &[]));
}

#[test]
fn test_find_deterministic_break_edge_picks_alphabetically_first() {
    let mut g = DependencyGraph::new();
    g.add_dependency("zebra", "apple");
    g.add_dependency("apple", "zebra");

    let scc: Vec<NodeIndex> = g.nodes.values().copied().collect();
    let edge = super::find_deterministic_break_edge(&g, &scc);

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
    let edge = super::find_deterministic_break_edge(&g, &scc);

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
    let edge = super::find_deterministic_break_edge(&g, &scc);

    assert!(
        edge.is_some(),
        "find_deterministic_break_edge should return the self-loop edge"
    );
    let (from, to) = edge.unwrap();
    assert_eq!(from, "a");
    assert_eq!(to, "a");
}
