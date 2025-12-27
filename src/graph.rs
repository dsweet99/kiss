
use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::violation::Violation;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Node;

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
        if from == to { return; }
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
    matches!(name, "main" | "lib" | "build" | "__main__" | "__init__" | "tests" | "conftest" | "setup")
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
        if !graph.paths.contains_key(module_name) { continue; }
        let metrics = graph.module_metrics(module_name);

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

fn extract_modules_from_import_from(child: Node, source: &str) -> Vec<String> {
    let mut modules = Vec::new();
    if let Some(m) = child.child_by_field_name("module_name") {
        let full_module = &source[m.start_byte()..m.end_byte()];
        let trimmed = full_module.trim_start_matches('.');
        if !trimmed.is_empty() { modules.push(top_level_module(trimmed)); }
    }
    let mut cursor = child.walk();
    for c in child.children(&mut cursor) {
        if c.kind() == "dotted_name" || c.kind() == "aliased_import" {
            let name_node = if c.kind() == "aliased_import" { c.child_by_field_name("name") } else { Some(c) };
            if let Some(n) = name_node {
                let name = &source[n.start_byte()..n.end_byte()];
                let trimmed = name.trim_start_matches('.');
                if !trimmed.is_empty() { modules.push(top_level_module(trimmed)); }
            }
        }
    }
    modules
}

fn extract_imports(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    extract_imports_recursive(node, source, &mut imports);
    imports
}

fn extract_imports_recursive(node: Node, source: &str, imports: &mut Vec<String>) {
    match node.kind() {
        "import_statement" => collect_import_names(node, source, imports),
        "import_from_statement" => imports.extend(extract_modules_from_import_from(node, source)),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_imports_recursive(child, source, imports);
            }
        }
    }
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
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;

    #[test]
    fn test_graph_imports_and_cycles() {
        let mut parser = create_parser().unwrap();
        assert!(extract_imports(parser.parse("import os", None).unwrap().root_node(), "import os").contains(&"os".into()));
        let code = "import os\ndef foo():\n    import json\n    from sys import argv";
        let mut nested = Vec::new();
        extract_imports_recursive(parser.parse(code, None).unwrap().root_node(), code, &mut nested);
        assert!(nested.contains(&"os".into()) && nested.contains(&"json".into()) && nested.contains(&"sys".into()));
        let mut g = DependencyGraph::default();
        g.add_dependency("a", "b"); g.add_dependency("b", "a");
        let cycle_info: CycleInfo = g.find_cycles();
        assert!(!cycle_info.cycles.is_empty());
        assert_eq!(g.get_or_create_node("test"), g.get_or_create_node("test"));
        g.add_dependency("x", "x");
        assert!(!g.nodes.contains_key("x"), "Self-edges ignored");
        let idx_a = *g.nodes.get("a").unwrap();
        let idx_b = *g.nodes.get("b").unwrap();
        assert!(g.is_cycle(&[idx_a, idx_b]) && !g.is_cycle(&[]) && !g.is_cycle(&[idx_a]));
        g.add_dependency("a", "c"); g.add_dependency("c", "d");
        let (trans, depth) = g.compute_transitive_and_depth(*g.nodes.get("a").unwrap());
        assert!(trans >= 2 && depth >= 2);
    }

    #[test]
    fn test_helpers_imports_and_complexity() {
        assert!(is_entry_point("main") && is_entry_point("test_foo") && !is_entry_point("utils"));
        assert!(is_orphan(&ModuleGraphMetrics::default(), "utils") && !is_orphan(&ModuleGraphMetrics { fan_in: 1, ..Default::default() }, "utils"));
        let mut g = DependencyGraph::new();
        g.paths.insert("foo".into(), PathBuf::from("src/foo.py"));
        assert_eq!(get_module_path(&g, "foo"), PathBuf::from("src/foo.py"));
        assert_eq!(top_level_module("foo.bar.baz"), "foo");
        let mut parser = create_parser().unwrap();
        let mods = extract_modules_from_import_from(parser.parse("from foo.bar import baz", None).unwrap().root_node().child(0).unwrap(), "from foo.bar import baz");
        assert!(mods.contains(&"foo".into()) && mods.contains(&"baz".into()));
        let rel = extract_modules_from_import_from(parser.parse("from ._export_format import X", None).unwrap().root_node().child(0).unwrap(), "from ._export_format import X");
        assert!(rel.contains(&"_export_format".into()), "Relative import: {rel:?}");
        assert!(is_decision_point("if_statement") && !is_decision_point("identifier"));
        assert_eq!(count_decision_points(parser.parse("if a:\n    if b:\n        pass", None).unwrap().root_node()), 2);
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "x = 1").unwrap();
        assert!(!module_name_from_path(&parse_file(&mut parser, tmp.path()).unwrap()).is_empty());
        assert!(compute_cyclomatic_complexity(parser.parse("def f():\n    if a:\n        pass", None).unwrap().root_node().child(0).unwrap()) >= 2);
    }
}
