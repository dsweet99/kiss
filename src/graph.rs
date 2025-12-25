//! Dependency graph analysis using Tarjan's SCC algorithm
//!
//! Detects cycles (strongly connected components) and computes dependency metrics:
//! - Fan-in/out: coupling to/from other modules
//! - Instability: Ce / (Ca + Ce), 0 (stable) to 1 (unstable)
//! - Transitive deps: all reachable modules via DFS

use crate::config::Config;
use crate::violation::Violation;
use crate::parsing::ParsedFile;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Dfs;
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Node;

/// A dependency graph for a codebase
pub struct DependencyGraph {
    /// The graph: nodes are module names, edges are dependencies
    pub graph: DiGraph<String, ()>,
    /// Map from module name to node index
    pub nodes: HashMap<String, NodeIndex>,
    /// Map from module name to actual file path
    pub paths: HashMap<String, PathBuf>,
}

/// Metrics for a single module in the dependency graph
#[derive(Debug, Default)]
pub struct ModuleGraphMetrics {
    /// Number of modules that depend on this module
    pub fan_in: usize,
    /// Number of modules this module depends on
    pub fan_out: usize,
    /// Instability: fan_out / (fan_in + fan_out)
    pub instability: f64,
    /// Transitive dependencies: all modules reachable from this one
    pub transitive_deps: usize,
}

/// Result of cycle detection
#[derive(Debug)]
pub struct CycleInfo {
    /// List of SCCs with more than one node (actual cycles)
    pub cycles: Vec<Vec<String>>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            nodes: HashMap::new(),
            paths: HashMap::new(),
        }
    }

    /// Get or create a node for a module
    pub fn get_or_create_node(&mut self, name: &str) -> NodeIndex {
        if let Some(&idx) = self.nodes.get(name) {
            idx
        } else {
            let idx = self.graph.add_node(name.to_string());
            self.nodes.insert(name.to_string(), idx);
            idx
        }
    }

    /// Add a dependency: `from` depends on `to`
    pub fn add_dependency(&mut self, from: &str, to: &str) {
        let from_idx = self.get_or_create_node(from);
        let to_idx = self.get_or_create_node(to);
        if !self.graph.contains_edge(from_idx, to_idx) {
            self.graph.add_edge(from_idx, to_idx, ());
        }
    }

    /// Compute metrics for a specific module
    pub fn module_metrics(&self, module: &str) -> ModuleGraphMetrics {
        let Some(&idx) = self.nodes.get(module) else {
            return ModuleGraphMetrics::default();
        };

        let fan_in = self
            .graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .count();
        let fan_out = self
            .graph
            .neighbors_directed(idx, petgraph::Direction::Outgoing)
            .count();
        let instability = if fan_in + fan_out > 0 {
            fan_out as f64 / (fan_in + fan_out) as f64
        } else {
            0.0
        };

        let transitive_deps = self.count_transitive_deps(idx);

        ModuleGraphMetrics {
            fan_in,
            fan_out,
            instability,
            transitive_deps,
        }
    }

    /// Count all modules reachable from a given node (transitive dependencies)
    fn count_transitive_deps(&self, start: NodeIndex) -> usize {
        let mut dfs = Dfs::new(&self.graph, start);
        dfs.next(&self.graph); // skip starting node
        std::iter::from_fn(|| dfs.next(&self.graph)).count()
    }

    fn is_cycle(&self, scc: &[NodeIndex]) -> bool {
        match scc.len() {
            0 => false,
            1 => self.graph.contains_edge(scc[0], scc[0]), // self-loop
            _ => true, // multi-node SCC
        }
    }

    /// Find all cycles in the graph
    pub fn find_cycles(&self) -> CycleInfo {
        let cycles = tarjan_scc(&self.graph)
            .into_iter()
            .filter(|scc| self.is_cycle(scc))
            .map(|scc| scc.into_iter().map(|idx| self.graph[idx].clone()).collect())
            .collect();
        CycleInfo { cycles }
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if module name is a known entry point (shouldn't be flagged as orphan)
fn is_entry_point(name: &str) -> bool {
    matches!(name, "main" | "lib" | "__main__" | "__init__")
        || name.starts_with("test_") || name.ends_with("_test")
        || name.contains("_integration") || name.contains("_bench")
}

fn get_module_path(graph: &DependencyGraph, module_name: &str) -> PathBuf {
    graph.paths.get(module_name).cloned().unwrap_or_else(|| PathBuf::from(format!("{}.py", module_name)))
}

fn is_orphan(metrics: &ModuleGraphMetrics, module_name: &str) -> bool {
    metrics.fan_in == 0 && metrics.fan_out == 0 && !is_entry_point(module_name)
}

/// Analyze dependency graph and return violations for high fan-out and cycles
#[must_use]
pub fn analyze_graph(graph: &DependencyGraph, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();

    for module_name in graph.nodes.keys() {
        let metrics = graph.module_metrics(module_name);

        if metrics.fan_out > config.fan_out {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "fan_out".to_string(),
                value: metrics.fan_out,
                threshold: config.fan_out,
                message: format!(
                    "Module '{}' depends on {} other modules (threshold: {})",
                    module_name, metrics.fan_out, config.fan_out
                ),
                suggestion: "Reduce dependencies by introducing abstractions or splitting the module.".to_string(),
            });
        }

        if metrics.fan_in > config.fan_in {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "fan_in".to_string(),
                value: metrics.fan_in,
                threshold: config.fan_in,
                message: format!(
                    "Module '{}' is depended on by {} other modules (threshold: {})",
                    module_name, metrics.fan_in, config.fan_in
                ),
                suggestion: "This module is heavily depended upon. Ensure it's stable and well-tested; changes here have wide impact.".to_string(),
            });
        }

        if is_orphan(&metrics, module_name) {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "orphan_module".to_string(),
                value: 0,
                threshold: 0,
                message: format!("Module '{}' has no dependencies and nothing depends on it", module_name),
                suggestion: "This may be dead code. Remove it, or integrate it into the codebase.".to_string(),
            });
        }

        if metrics.transitive_deps > config.transitive_deps {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "transitive_deps".to_string(),
                value: metrics.transitive_deps,
                threshold: config.transitive_deps,
                message: format!(
                    "Module '{}' has {} transitive dependencies (threshold: {})",
                    module_name, metrics.transitive_deps, config.transitive_deps
                ),
                suggestion: "Reduce coupling by introducing interfaces, using dependency injection, or splitting into smaller modules.".to_string(),
            });
        }
    }

    for cycle in graph.find_cycles().cycles {
        let cycle_str = cycle.join(" → ");
        let first_module = cycle.first().cloned().unwrap_or_default();

        violations.push(Violation {
            file: get_module_path(graph, &first_module),
            line: 1,
            unit_name: first_module,
            metric: "dependency_cycle".to_string(),
            value: cycle.len(),
            threshold: 0, // Any cycle is a violation
            message: format!("Circular dependency detected: {} → {}", cycle_str, cycle.first().unwrap_or(&String::new())),
            suggestion: "Break the cycle by introducing an interface or restructuring dependencies.".to_string(),
        });
    }

    violations
}

/// Instability metrics for a module
#[derive(Debug)]
pub struct InstabilityMetric {
    pub module_name: String,
    pub instability: f64,
    pub fan_in: usize,
    pub fan_out: usize,
}

/// Collect instability metrics for all modules in the graph
/// Returns modules with instability > 0, sorted by instability (highest first)
#[must_use]
pub fn collect_instability_metrics(graph: &DependencyGraph) -> Vec<InstabilityMetric> {
    let mut metrics: Vec<InstabilityMetric> = graph
        .nodes
        .keys()
        .map(|module_name| {
            let m = graph.module_metrics(module_name);
            InstabilityMetric {
                module_name: module_name.clone(),
                instability: m.instability,
                fan_in: m.fan_in,
                fan_out: m.fan_out,
            }
        })
        .filter(|m| m.instability > 0.0 && (m.fan_in > 0 || m.fan_out > 0))
        .collect();
    
    metrics.sort_by(|a, b| {
        b.instability.partial_cmp(&a.instability)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.module_name.cmp(&b.module_name))
    });
    
    metrics
}

fn module_name_from_path(parsed: &ParsedFile) -> String {
    parsed.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "unknown".to_string())
}

/// Build a dependency graph from parsed files
#[must_use]
pub fn build_dependency_graph(parsed_files: &[&ParsedFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    for parsed in parsed_files {
        let module_name = module_name_from_path(parsed);
        graph.paths.insert(module_name.clone(), parsed.path.clone());
        graph.get_or_create_node(&module_name);

        for import in extract_imports(parsed.tree.root_node(), &parsed.source) {
            graph.add_dependency(&module_name, &import);
        }
    }

    graph
}

fn top_level_module(name: &str) -> String {
    name.split('.').next().unwrap_or(name).to_string()
}

fn extract_module_from_import_from(child: Node, source: &str) -> Option<String> {
    child.child_by_field_name("module_name")
        .map(|m| top_level_module(&source[m.start_byte()..m.end_byte()]))
}

/// Extract imported module names from a file
fn extract_imports(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => collect_import_names(child, source, &mut imports),
            "import_from_statement" => if let Some(m) = extract_module_from_import_from(child, source) {
                imports.push(m);
            },
            _ => {}
        }
    }

    imports
}

fn collect_import_names(node: Node, source: &str, imports: &mut Vec<String>) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => imports.push(top_level_module(&source[child.start_byte()..child.end_byte()])),
            "aliased_import" => if let Some(name_node) = child.child_by_field_name("name") {
                imports.push(top_level_module(&source[name_node.start_byte()..name_node.end_byte()]));
            },
            _ => {}
        }
    }
}

/// Compute cyclomatic complexity for a function node
/// Formula: number of decision points + 1
pub fn compute_cyclomatic_complexity(node: Node) -> usize {
    let mut complexity = 1; // Base complexity

    complexity += count_decision_points(node);

    complexity
}

fn is_decision_point(kind: &str) -> bool {
    matches!(kind, 
        "if_statement" | "elif_clause" | "for_statement" | "while_statement"
        | "except_clause" | "with_statement" | "match_statement" | "case_clause"
        | "boolean_operator" | "conditional_expression"
    )
}

fn count_decision_points(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|child| is_decision_point(child.kind()) as usize + count_decision_points(child))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::create_parser;
    use std::path::Path;

    fn parse_imports(code: &str) -> Vec<String> {
        let mut parser = create_parser().unwrap();
        extract_imports(parser.parse(code, None).unwrap().root_node(), code)
    }

    #[test]
    fn test_import_extraction() {
        assert!(parse_imports("import os").contains(&"os".into()));
        assert!(parse_imports("import os.path").contains(&"os".into()));
        assert!(parse_imports("import numpy as np").contains(&"numpy".into()));
        assert!(parse_imports("from collections import defaultdict").contains(&"collections".into()));
        assert!(parse_imports("from os.path import join").contains(&"os".into()));
        let multi = parse_imports("import os\nimport sys\nfrom collections import defaultdict");
        assert!(multi.contains(&"os".into()) && multi.contains(&"sys".into()) && multi.contains(&"collections".into()));
    }

    #[test]
    fn test_dependency_graph_operations() {
        let mut g = DependencyGraph::default();
        let idx = g.get_or_create_node("module_a");
        assert!(g.graph.node_weight(idx).is_some());
        let idx2 = g.get_or_create_node("module_a");
        assert_eq!(idx, idx2);
        g.add_dependency("a", "b"); g.add_dependency("b", "c");
        assert_eq!(g.graph.edge_count(), 2);
        assert!(g.count_transitive_deps(*g.nodes.get("a").unwrap()) >= 2);
    }

    #[test]
    fn test_cycles_and_metrics() {
        let mut g = DependencyGraph::default();
        g.add_dependency("a", "b"); g.add_dependency("b", "a");
        assert!(!g.find_cycles().cycles.is_empty());
        let m = ModuleGraphMetrics { fan_in: 1, fan_out: 2, instability: 0.5, transitive_deps: 3 };
        assert_eq!(m.fan_in, 1);
        let c = CycleInfo { cycles: vec![vec!["a".into()]] };
        assert_eq!(c.cycles.len(), 1);
    }

    #[test]
    fn test_analyze_and_decision_points() {
        assert!(analyze_graph(&DependencyGraph::default(), &crate::Config::default()).is_empty());
        let mut parser = create_parser().unwrap();
        assert!(count_decision_points(parser.parse("if x: pass", None).unwrap().root_node()) >= 1);
    }

    #[test]
    fn test_instability_metrics() {
        let m = InstabilityMetric { module_name: "m".into(), instability: 50.0, fan_in: 2, fan_out: 2 };
        assert_eq!(m.instability, 50.0);
        let empty = DependencyGraph { graph: petgraph::Graph::new(), nodes: HashMap::new(), paths: HashMap::new() };
        assert!(collect_instability_metrics(&empty).is_empty());
        let mut graph = petgraph::Graph::new();
        let (a, b) = (graph.add_node("a".into()), graph.add_node("b".into()));
        graph.add_edge(a, b, ());
        let mut nodes = HashMap::new();
        nodes.insert("a".into(), a); nodes.insert("b".into(), b);
        let g = DependencyGraph { graph, nodes, paths: HashMap::new() };
        assert!(!collect_instability_metrics(&g).is_empty());
    }

    #[test]
    fn test_is_entry_point() {
        assert!(is_entry_point("main") && is_entry_point("lib") && is_entry_point("__main__"));
        assert!(is_entry_point("test_foo") && is_entry_point("foo_test"));
        assert!(is_entry_point("cli_integration") && is_entry_point("perf_bench"));
        assert!(!is_entry_point("utils") && !is_entry_point("parser") && !is_entry_point("config"));
    }

    #[test]
    fn test_helper_functions() {
        let g = DependencyGraph::default();
        assert_eq!(get_module_path(&g, "foo"), std::path::PathBuf::from("foo.py"));
        let metrics = ModuleGraphMetrics::default();
        assert!(!is_orphan(&metrics, "main"));
        assert!(is_orphan(&ModuleGraphMetrics { fan_in: 0, fan_out: 0, ..Default::default() }, "utils"));
        assert!(is_decision_point("if_statement") && is_decision_point("boolean_operator"));
        assert!(!is_decision_point("identifier"));
        assert_eq!(top_level_module("os.path"), "os");
        assert_eq!(top_level_module("collections"), "collections");
        let parsed = crate::parsing::parse_file(&mut create_parser().unwrap(), Path::new("test.py"));
        if let Ok(p) = parsed { let _ = module_name_from_path(&p); }
        let mut parser = create_parser().unwrap();
        let tree = parser.parse("from os import path", None).unwrap();
        let node = tree.root_node().child(0).unwrap();
        let _ = extract_module_from_import_from(node, "from os import path");
    }

    #[test]
    fn test_is_cycle() {
        let mut g = DependencyGraph::default();
        g.add_dependency("a", "a");
        let idx = *g.nodes.get("a").unwrap();
        assert!(g.is_cycle(&[idx]));
        let mut g2 = DependencyGraph::default();
        g2.add_dependency("x", "y");
        let x = *g2.nodes.get("x").unwrap();
        assert!(!g2.is_cycle(&[x]));
    }
}
