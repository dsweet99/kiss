//! Dependency graph analysis

use crate::config::Config;
use crate::counts::Violation;
use crate::parsing::ParsedFile;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
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
    fn get_or_create_node(&mut self, name: &str) -> NodeIndex {
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
        // Avoid duplicate edges
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

        ModuleGraphMetrics {
            fan_in,
            fan_out,
            instability,
        }
    }

    /// Find all cycles in the graph
    pub fn find_cycles(&self) -> CycleInfo {
        let sccs = tarjan_scc(&self.graph);

        let cycles: Vec<Vec<String>> = sccs
            .into_iter()
            .filter(|scc| {
                if scc.len() > 1 {
                    // Multi-node SCC is always a cycle
                    true
                } else if scc.len() == 1 {
                    // Single-node SCC is a cycle only if it has a self-loop
                    let idx = scc[0];
                    self.graph.contains_edge(idx, idx)
                } else {
                    false
                }
            })
            .map(|scc| {
                scc.into_iter()
                    .map(|idx| self.graph[idx].clone())
                    .collect()
            })
            .collect();

        CycleInfo { cycles }
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Analyze dependency graph and return violations for high fan-out and cycles
pub fn analyze_graph(graph: &DependencyGraph, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();

    // Helper to get actual file path or synthesize one
    let get_path = |module_name: &str| -> PathBuf {
        graph.paths.get(module_name)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(format!("{}.py", module_name)))
    };

    // Check fan-out for each module
    for module_name in graph.nodes.keys() {
        let metrics = graph.module_metrics(module_name);

        if metrics.fan_out > config.fan_out {
            violations.push(Violation {
                file: get_path(module_name),
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
                file: get_path(module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "fan_in".to_string(),
                value: metrics.fan_in,
                threshold: config.fan_in,
                message: format!(
                    "Module '{}' is depended on by {} other modules (threshold: {})",
                    module_name, metrics.fan_in, config.fan_in
                ),
                suggestion: "Consider if this module has too many responsibilities; split if needed.".to_string(),
            });
        }
    }

    // Check for cycles
    let cycle_info = graph.find_cycles();
    for cycle in cycle_info.cycles {
        let cycle_str = cycle.join(" → ");
        let first_module = cycle.first().cloned().unwrap_or_default();

        violations.push(Violation {
            file: get_path(&first_module),
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

/// Build a dependency graph from parsed files
pub fn build_dependency_graph(parsed_files: &[&ParsedFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    for parsed in parsed_files {
        let module_name = parsed
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string());

        // Store the actual file path for this module
        graph.paths.insert(module_name.clone(), parsed.path.clone());

        // Ensure the module exists in the graph even if it has no dependencies
        graph.get_or_create_node(&module_name);

        // Extract imports
        let root = parsed.tree.root_node();
        let imports = extract_imports(root, &parsed.source);

        for import in imports {
            graph.add_dependency(&module_name, &import);
        }
    }

    graph
}

/// Extract imported module names from a file
fn extract_imports(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                // import foo, bar
                collect_import_names(child, source, &mut imports);
            }
            "import_from_statement" => {
                // from foo import bar
                if let Some(module) = child.child_by_field_name("module_name") {
                    let name = source[module.start_byte()..module.end_byte()].to_string();
                    // Take the top-level module name
                    let top_level = name.split('.').next().unwrap_or(&name).to_string();
                    imports.push(top_level);
                }
            }
            _ => {}
        }
    }

    imports
}

fn collect_import_names(node: Node, source: &str, imports: &mut Vec<String>) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                let name = source[child.start_byte()..child.end_byte()].to_string();
                let top_level = name.split('.').next().unwrap_or(&name).to_string();
                imports.push(top_level);
            }
            "aliased_import" => {
                // import foo as bar
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = source[name_node.start_byte()..name_node.end_byte()].to_string();
                    let top_level = name.split('.').next().unwrap_or(&name).to_string();
                    imports.push(top_level);
                }
            }
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

fn count_decision_points(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_statement" | "elif_clause" | "for_statement" | "while_statement"
            | "except_clause" | "with_statement" | "match_statement" | "case_clause" => {
                count += 1;
            }
            "boolean_operator" => {
                // Each `and` or `or` adds a decision point
                count += 1;
            }
            "conditional_expression" => {
                // Ternary operator: x if cond else y
                count += 1;
            }
            _ => {}
        }
        count += count_decision_points(child);
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::create_parser;

    fn parse_and_extract_imports(code: &str) -> Vec<String> {
        let mut parser = create_parser().unwrap();
        let tree = parser.parse(code, None).unwrap();
        extract_imports(tree.root_node(), code)
    }

    #[test]
    fn extracts_simple_import() {
        let imports = parse_and_extract_imports("import os");
        assert!(imports.contains(&"os".to_string()), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_dotted_import() {
        let imports = parse_and_extract_imports("import os.path");
        // Should extract top-level module "os"
        assert!(imports.contains(&"os".to_string()), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_aliased_import() {
        let imports = parse_and_extract_imports("import numpy as np");
        assert!(imports.contains(&"numpy".to_string()), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_from_import() {
        let imports = parse_and_extract_imports("from collections import defaultdict");
        assert!(
            imports.contains(&"collections".to_string()),
            "Expected 'collections' in imports, got: {:?}",
            imports
        );
    }

    #[test]
    fn extracts_from_dotted_import() {
        let imports = parse_and_extract_imports("from os.path import join");
        // Should extract top-level module "os"
        assert!(imports.contains(&"os".to_string()), "imports: {:?}", imports);
    }

    #[test]
    fn extracts_multiple_imports() {
        let code = r#"
import os
import sys
from collections import defaultdict
from pathlib import Path
"#;
        let imports = parse_and_extract_imports(code);
        assert!(imports.contains(&"os".to_string()), "imports: {:?}", imports);
        assert!(imports.contains(&"sys".to_string()), "imports: {:?}", imports);
        assert!(imports.contains(&"collections".to_string()), "imports: {:?}", imports);
        assert!(imports.contains(&"pathlib".to_string()), "imports: {:?}", imports);
    }
}

