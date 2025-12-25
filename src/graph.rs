//! Dependency graph analysis using Tarjan's SCC algorithm
//!
//! Detects cycles (strongly connected components) and computes dependency metrics:
//! - Fan-in/out: coupling to/from other modules

use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::violation::Violation;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Node;

/// A dependency graph for a codebase
pub struct DependencyGraph {
    pub graph: DiGraph<String, ()>,
    pub nodes: HashMap<String, NodeIndex>,
    pub paths: HashMap<String, PathBuf>,
}

#[derive(Debug, Default)]
pub struct ModuleGraphMetrics {
    pub fan_in: usize,
    pub fan_out: usize,
    pub transitive_dependencies: usize,
    pub dependency_depth: usize,
}

#[derive(Debug)]
pub struct CycleInfo {
    pub cycles: Vec<Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self { graph: DiGraph::new(), nodes: HashMap::new(), paths: HashMap::new() }
    }

    pub fn get_or_create_node(&mut self, name: &str) -> NodeIndex {
        if let Some(&idx) = self.nodes.get(name) { idx }
        else { let idx = self.graph.add_node(name.to_string()); self.nodes.insert(name.to_string(), idx); idx }
    }

    pub fn add_dependency(&mut self, from: &str, to: &str) {
        let from_idx = self.get_or_create_node(from);
        let to_idx = self.get_or_create_node(to);
        if !self.graph.contains_edge(from_idx, to_idx) { self.graph.add_edge(from_idx, to_idx, ()); }
    }

    pub fn module_metrics(&self, module: &str) -> ModuleGraphMetrics {
        let Some(&idx) = self.nodes.get(module) else { return ModuleGraphMetrics::default(); };
        let (transitive, depth) = self.compute_transitive_and_depth(idx);
        ModuleGraphMetrics {
            fan_in: self.graph.neighbors_directed(idx, petgraph::Direction::Incoming).count(),
            fan_out: self.graph.neighbors_directed(idx, petgraph::Direction::Outgoing).count(),
            transitive_dependencies: transitive,
            dependency_depth: depth,
        }
    }

    fn compute_transitive_and_depth(&self, start: NodeIndex) -> (usize, usize) {
        use std::collections::{HashSet, VecDeque};
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut max_depth = 0;
        queue.push_back((start, 0));
        while let Some((node, depth)) = queue.pop_front() {
            for neighbor in self.graph.neighbors_directed(node, petgraph::Direction::Outgoing) {
                if visited.insert(neighbor) {
                    max_depth = max_depth.max(depth + 1);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
        (visited.len(), max_depth)
    }

    fn is_cycle(&self, scc: &[NodeIndex]) -> bool {
        match scc.len() { 0 => false, 1 => self.graph.contains_edge(scc[0], scc[0]), _ => true }
    }

    pub fn find_cycles(&self) -> CycleInfo {
        CycleInfo {
            cycles: tarjan_scc(&self.graph).into_iter()
                .filter(|scc| self.is_cycle(scc))
                .map(|scc| scc.into_iter().map(|idx| self.graph[idx].clone()).collect())
                .collect(),
        }
    }
}

impl Default for DependencyGraph {
    fn default() -> Self { Self::new() }
}

fn is_entry_point(name: &str) -> bool {
    matches!(name, "main" | "lib" | "__main__" | "__init__")
        || name.starts_with("test_") || name.ends_with("_test")
        || name.contains("_integration") || name.contains("_bench")
}

fn get_module_path(graph: &DependencyGraph, module_name: &str) -> PathBuf {
    graph.paths.get(module_name).cloned().unwrap_or_else(|| PathBuf::from(format!("{module_name}.py")))
}

fn is_orphan(metrics: &ModuleGraphMetrics, module_name: &str) -> bool {
    metrics.fan_in == 0 && metrics.fan_out == 0 && !is_entry_point(module_name)
}

#[must_use]
pub fn analyze_graph(graph: &DependencyGraph, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    for module_name in graph.nodes.keys() {
        if !graph.paths.contains_key(module_name) { continue; } // Skip external deps
        let metrics = graph.module_metrics(module_name);

        if metrics.fan_out > config.fan_out {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "fan_out".to_string(), value: metrics.fan_out, threshold: config.fan_out,
                message: format!("Module '{}' depends on {} other modules (threshold: {})", module_name, metrics.fan_out, config.fan_out),
                suggestion: "Reduce dependencies by introducing abstractions or splitting the module.".to_string(),
            });
        }
        if metrics.fan_in > config.fan_in {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "fan_in".to_string(), value: metrics.fan_in, threshold: config.fan_in,
                message: format!("Module '{}' is depended on by {} other modules (threshold: {})", module_name, metrics.fan_in, config.fan_in),
                suggestion: "This module is heavily depended upon. Ensure it's stable and well-tested; changes here have wide impact.".to_string(),
            });
        }
        if is_orphan(&metrics, module_name) {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "orphan_module".to_string(), value: 0, threshold: 0,
                message: format!("Module '{module_name}' has no dependencies and nothing depends on it"),
                suggestion: "This may be dead code. Remove it, or integrate it into the codebase.".to_string(),
            });
        }
        if metrics.transitive_dependencies > config.transitive_dependencies {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "transitive_dependencies".to_string(), value: metrics.transitive_dependencies, threshold: config.transitive_dependencies,
                message: format!("Module '{}' has {} transitive dependencies (threshold: {})", module_name, metrics.transitive_dependencies, config.transitive_dependencies),
                suggestion: "Reduce coupling by introducing abstraction layers or splitting responsibilities.".to_string(),
            });
        }
        if metrics.dependency_depth > config.dependency_depth {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "dependency_depth".to_string(), value: metrics.dependency_depth, threshold: config.dependency_depth,
                message: format!("Module '{}' has dependency depth {} (threshold: {})", module_name, metrics.dependency_depth, config.dependency_depth),
                suggestion: "Flatten the dependency chain by moving shared logic to a common base layer.".to_string(),
            });
        }
    }
    for cycle in graph.find_cycles().cycles {
        let cycle_str = cycle.join(" → ");
        let first_module = cycle.first().cloned().unwrap_or_default();
        violations.push(Violation {
            file: get_module_path(graph, &first_module), line: 1, unit_name: first_module.clone(),
            metric: "dependency_cycle".to_string(), value: cycle.len(), threshold: 0,
            message: format!("Circular dependency detected: {} → {}", cycle_str, cycle.first().unwrap_or(&String::new())),
            suggestion: "Break the cycle by introducing an interface or restructuring dependencies.".to_string(),
        });
        if cycle.len() > config.cycle_size {
            violations.push(Violation {
                file: get_module_path(graph, &first_module), line: 1, unit_name: first_module,
                metric: "cycle_size".to_string(), value: cycle.len(), threshold: config.cycle_size,
                message: format!("Dependency cycle has {} modules (threshold: {})", cycle.len(), config.cycle_size),
                suggestion: "Large cycles are harder to untangle. Prioritize breaking this cycle into smaller pieces.".to_string(),
            });
        }
    }
    violations
}

fn module_name_from_path(parsed: &ParsedFile) -> String {
    parsed.path.file_stem().map_or_else(|| "unknown".to_string(), |s| s.to_string_lossy().into_owned())
}

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

fn top_level_module(name: &str) -> String { name.split('.').next().unwrap_or(name).to_string() }

fn extract_module_from_import_from(child: Node, source: &str) -> Option<String> {
    child.child_by_field_name("module_name").map(|m| top_level_module(&source[m.start_byte()..m.end_byte()]))
}

fn extract_imports(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => collect_import_names(child, source, &mut imports),
            "import_from_statement" => if let Some(m) = extract_module_from_import_from(child, source) { imports.push(m); },
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
            "aliased_import" => if let Some(n) = child.child_by_field_name("name") {
                imports.push(top_level_module(&source[n.start_byte()..n.end_byte()]));
            },
            _ => {}
        }
    }
}

pub fn compute_cyclomatic_complexity(node: Node) -> usize { 1 + count_decision_points(node) }

fn is_decision_point(kind: &str) -> bool {
    matches!(kind, "if_statement" | "elif_clause" | "for_statement" | "while_statement"
        | "except_clause" | "with_statement" | "match_statement" | "case_clause"
        | "boolean_operator" | "conditional_expression")
}

fn count_decision_points(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).map(|c| usize::from(is_decision_point(c.kind())) + count_decision_points(c)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::create_parser;

    fn parse_imports(code: &str) -> Vec<String> {
        let mut parser = create_parser().unwrap();
        extract_imports(parser.parse(code, None).unwrap().root_node(), code)
    }

    #[test]
    fn test_import_extraction() {
        assert!(parse_imports("import os").contains(&"os".into()));
        assert!(parse_imports("from collections import defaultdict").contains(&"collections".into()));
    }

    #[test]
    fn test_graph_ops() {
        let mut g = DependencyGraph::default();
        g.add_dependency("a", "b"); g.add_dependency("b", "a");
        assert!(!g.find_cycles().cycles.is_empty());
        assert_eq!(g.module_metrics("a").fan_out, 1);
    }

    #[test]
    fn test_entry_points() {
        assert!(is_entry_point("main") && is_entry_point("test_foo") && is_entry_point("cli_integration"));
        assert!(!is_entry_point("utils"));
    }

    #[test]
    fn test_orphan() {
        assert!(is_orphan(&ModuleGraphMetrics { fan_in: 0, fan_out: 0, ..Default::default() }, "utils"));
        assert!(!is_orphan(&ModuleGraphMetrics { fan_in: 1, fan_out: 0, ..Default::default() }, "utils"));
        assert!(!is_orphan(&ModuleGraphMetrics { fan_in: 0, fan_out: 0, ..Default::default() }, "main"));
    }

    #[test]
    fn test_cyclomatic_complexity() {
        let mut parser = create_parser().unwrap();
        let tree = parser.parse("def f():\n    if a:\n        pass", None).unwrap();
        assert!(compute_cyclomatic_complexity(tree.root_node().child(0).unwrap()) >= 2);
    }
}
