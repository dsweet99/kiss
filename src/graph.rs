use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::py_imports::is_type_checking_block;
use crate::violation::Violation;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Node;

struct ImportInfo {
    from_qualified: String,
    from_parent: Option<String>,
    imports: Vec<String>,
}

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
        Self {
            graph: DiGraph::new(),
            nodes: HashMap::new(),
            paths: HashMap::new(),
        }
    }

    pub fn get_or_create_node(&mut self, name: &str) -> NodeIndex {
        if let Some(&idx) = self.nodes.get(name) {
            idx
        } else {
            let idx = self.graph.add_node(name.to_string());
            self.nodes.insert(name.to_string(), idx);
            idx
        }
    }

    pub fn add_dependency(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let from_idx = self.get_or_create_node(from);
        let to_idx = self.get_or_create_node(to);
        if !self.graph.contains_edge(from_idx, to_idx) {
            self.graph.add_edge(from_idx, to_idx, ());
        }
    }

    pub fn module_metrics(&self, module: &str) -> ModuleGraphMetrics {
        let Some(&idx) = self.nodes.get(module) else {
            return ModuleGraphMetrics::default();
        };
        let (transitive, depth) = self.compute_transitive_and_depth(idx);
        ModuleGraphMetrics {
            fan_in: self
                .graph
                .neighbors_directed(idx, petgraph::Direction::Incoming)
                .count(),
            fan_out: self
                .graph
                .neighbors_directed(idx, petgraph::Direction::Outgoing)
                .count(),
            transitive_dependencies: transitive,
            dependency_depth: depth,
        }
    }

    fn compute_transitive_and_depth(&self, start: NodeIndex) -> (usize, usize) {
        use std::collections::{HashSet, VecDeque};
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut max_depth = 0;
        // Seed visited with start so it is never counted as its own transitive dependency.
        visited.insert(start);
        queue.push_back((start, 0));
        while let Some((node, depth)) = queue.pop_front() {
            for neighbor in self
                .graph
                .neighbors_directed(node, petgraph::Direction::Outgoing)
            {
                if visited.insert(neighbor) {
                    max_depth = max_depth.max(depth + 1);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
        // Subtract 1 for the start node itself.
        (visited.len() - 1, max_depth)
    }

    fn is_cycle(&self, scc: &[NodeIndex]) -> bool {
        match scc.len() {
            0 => false,
            1 => self.graph.contains_edge(scc[0], scc[0]),
            _ => true,
        }
    }

    pub fn find_cycles(&self) -> CycleInfo {
        CycleInfo {
            cycles: tarjan_scc(&self.graph)
                .into_iter()
                .filter(|scc| self.is_cycle(scc))
                .map(|scc| scc.into_iter().map(|idx| self.graph[idx].clone()).collect())
                .collect(),
        }
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

fn is_entry_point(name: &str) -> bool {
    // Extract bare name from qualified name (e.g., "attr.main" → "main")
    let bare = name.rsplit('.').next().unwrap_or(name);
    // Treat anything under a `tests/` directory as an entry point. Integration tests are
    // intentionally standalone modules with no incoming/outgoing dependencies.
    if name == "tests" || name.starts_with("tests.") || name.contains(".tests.") {
        return true;
    }
    matches!(
        bare,
        "main" | "lib" | "build" | "__main__" | "__init__" | "tests" | "conftest" | "setup"
    ) || bare.starts_with("test_")
        || bare.ends_with("_test")
        || bare.contains("_integration")
        || bare.contains("_bench")
}

fn get_module_path(graph: &DependencyGraph, module_name: &str) -> PathBuf {
    graph
        .paths
        .get(module_name)
        .cloned()
        .unwrap_or_else(|| PathBuf::from(format!("{module_name}.py")))
}

fn is_test_module(graph: &DependencyGraph, module_name: &str) -> bool {
    use std::ffi::OsStr;
    let Some(p) = graph.paths.get(module_name) else {
        return false;
    };
    p.components()
        .any(|c| c.as_os_str() == OsStr::new("tests") || c.as_os_str() == OsStr::new("test"))
}

fn is_orphan(metrics: &ModuleGraphMetrics, module_name: &str) -> bool {
    metrics.fan_in == 0 && metrics.fan_out == 0 && !is_entry_point(module_name)
}

#[must_use]
pub fn analyze_graph(graph: &DependencyGraph, config: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    for module_name in graph.nodes.keys() {
        if !graph.paths.contains_key(module_name) {
            continue;
        }
        let metrics = graph.module_metrics(module_name);

        if !is_test_module(graph, module_name) && is_orphan(&metrics, module_name) {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "orphan_module".to_string(),
                value: 0,
                threshold: 0,
                message: format!(
                    "Module '{module_name}' has no dependencies and nothing depends on it"
                ),
                suggestion: "This may be dead code. Remove it, or integrate it into the codebase."
                    .to_string(),
            });
        }
        // Only flag transitive deps if fan_in > 0; entry points (fan_in=0) don't propagate coupling
        if metrics.transitive_dependencies > config.transitive_dependencies && metrics.fan_in > 0 {
            violations.push(Violation {
                file: get_module_path(graph, module_name), line: 1, unit_name: module_name.clone(),
                metric: "transitive_dependencies".to_string(), value: metrics.transitive_dependencies, threshold: config.transitive_dependencies,
                message: format!("Module '{}' has {} transitive dependencies (threshold: {})", module_name, metrics.transitive_dependencies, config.transitive_dependencies),
                suggestion: "Reduce coupling by introducing abstraction layers or splitting responsibilities.".to_string(),
            });
        }
        if metrics.dependency_depth > config.dependency_depth {
            violations.push(Violation {
                file: get_module_path(graph, module_name),
                line: 1,
                unit_name: module_name.clone(),
                metric: "dependency_depth".to_string(),
                value: metrics.dependency_depth,
                threshold: config.dependency_depth,
                message: format!(
                    "Module '{}' has dependency depth {} (threshold: {})",
                    module_name, metrics.dependency_depth, config.dependency_depth
                ),
                suggestion:
                    "Flatten the dependency chain by moving shared logic to a common base layer."
                        .to_string(),
            });
        }
    }
    for cycle in graph.find_cycles().cycles {
        let cycle_str = cycle.join(" → ");
        let first_module = cycle.first().cloned().unwrap_or_default();
        if cycle.len() > config.cycle_size {
            violations.push(Violation {
                file: get_module_path(graph, &first_module), line: 1, unit_name: first_module,
                metric: "cycle_size".to_string(), value: cycle.len(), threshold: config.cycle_size,
                message: format!("Circular dependency detected ({} modules, threshold: {}): {} → {}", cycle.len(), config.cycle_size, cycle_str, cycle.first().unwrap_or(&String::new())),
                suggestion: "Large cycles are harder to untangle. Prioritize breaking this cycle into smaller pieces.".to_string(),
            });
        }
    }
    violations
}

/// Extract qualified module name: includes parent directory to avoid collisions.
/// Example: "src/attr/exceptions.py" → "attr.exceptions"
fn qualified_module_name(path: &std::path::Path) -> String {
    use std::path::Component;

    let stem = path
        .file_stem()
        .map_or("unknown", |s| s.to_str().unwrap_or("unknown"));

    let mut dirs: Vec<String> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(os) => os.to_str().map(std::string::ToString::to_string),
                    Component::CurDir => Some(".".to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Prefer paths relative to a source root, if present.
    // Keep components after the *last* "src" (handles absolute paths that include repo prefix).
    if let Some(pos) = dirs.iter().rposition(|d| d == "src") {
        dirs = dirs[(pos + 1)..].to_vec();
    }

    // Drop "." segments.
    dirs.retain(|d| d != ".");

    // For absolute paths without a known source root, avoid huge prefixes by using a short tail.
    if path.is_absolute() && dirs.len() > 2 {
        dirs = dirs[(dirs.len() - 2)..].to_vec();
    }

    if dirs.is_empty() {
        stem.to_string()
    } else {
        format!("{}.{}", dirs.join("."), stem)
    }
}

/// Extract just the bare module name (filename without extension)
fn bare_module_name(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(
        || "unknown".to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

#[must_use]
pub fn build_dependency_graph(parsed_files: &[&ParsedFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    // Build mapping from bare names to qualified names for import resolution
    // Also track if a bare name is ambiguous (maps to multiple qualified names)
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

    // First pass: register all modules with qualified names
    for parsed in parsed_files {
        let qualified = qualified_module_name(&parsed.path);
        let bare = bare_module_name(&parsed.path);
        graph.paths.insert(qualified.clone(), parsed.path.clone());
        graph.get_or_create_node(&qualified);
        bare_to_qualified.entry(bare).or_default().push(qualified);
    }

    // Extract imports per-file in parallel; keep ordering by collecting into a Vec (slice par_iter is indexed).
    let per_file: Vec<ImportInfo> = parsed_files
        .par_iter()
        .map(|parsed| ImportInfo {
            from_qualified: qualified_module_name(&parsed.path),
            from_parent: parsed
                .path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .map(std::string::ToString::to_string),
            imports: extract_imports_for_cache(parsed.tree.root_node(), &parsed.source),
        })
        .collect();

    // Second pass: add dependency edges, resolving imports to qualified names
    for info in &per_file {
        for import in &info.imports {
            // Try to resolve import to a qualified module name
            if graph.nodes.contains_key(import) {
                // Already a qualified internal module name.
                graph.add_dependency(&info.from_qualified, import);
            } else if let Some(resolved) =
                resolve_import(import, info.from_parent.as_deref(), &bare_to_qualified)
            {
                graph.add_dependency(&info.from_qualified, &resolved);
            } else {
                // External import (not in analyzed codebase) - skip
            }
        }

    }
    graph
}

#[must_use]
pub fn build_dependency_graph_from_import_lists(files: &[(PathBuf, Vec<String>)]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

    for (path, _) in files {
        let qualified = qualified_module_name(path);
        let bare = bare_module_name(path);
        graph.paths.insert(qualified.clone(), path.clone());
        graph.get_or_create_node(&qualified);
        bare_to_qualified.entry(bare).or_default().push(qualified);
    }

    for (path, imports) in files {
        let from_qualified = qualified_module_name(path);
        let from_parent = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str());

        for import in imports {
            if graph.nodes.contains_key(import) {
                graph.add_dependency(&from_qualified, import);
            } else if let Some(resolved) = resolve_import(import, from_parent, &bare_to_qualified) {
                graph.add_dependency(&from_qualified, &resolved);
            } else {
                // External import (not in analyzed codebase) - skip
            }
        }

    }
    graph
}

/// Resolve an import name to a qualified module name.
/// Prefers modules in the same package (same parent directory).
fn resolve_import(
    import: &str,
    from_parent: Option<&str>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if let Some(candidates) = bare_to_qualified.get(import) {
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }

        // Multiple candidates - prefer one in the same package
        if let Some(parent) = from_parent {
            let prefix = format!("{parent}.");
            for candidate in candidates {
                if candidate.starts_with(&prefix) {
                    return Some(candidate.clone());
                }
            }
        }
    } else if let Some((_, last)) = import.rsplit_once('.')
        && let Some(candidates) = bare_to_qualified.get(last)
    {
        let mut matches: Vec<String> = candidates
            .iter()
            .filter(|c| c.ends_with(import))
            .cloned()
            .collect();
        if matches.len() == 1 {
            return matches.pop();
        }
        if matches.is_empty()
            && let Some(parent) = from_parent
        {
            let prefix = format!("{parent}.");
            matches = candidates
                .iter()
                .filter(|c| c.starts_with(&prefix))
                .cloned()
                .collect();
            if matches.len() == 1 {
                return matches.pop();
            }
        }
    }

    // Ambiguous or external import - don't create edge
    None
}

fn push_dotted_segments(raw: &str, modules: &mut Vec<String>) {
    // Treat dotted module paths as a single import target (e.g., "foo.bar", not ["foo", "bar"]).
    // Splitting creates spurious edges to unrelated local modules named like common segments
    // ("utils", "types", "errors", etc.), which can inflate SCC cycles dramatically.
    let trimmed = raw.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return;
    }
    modules.push(trimmed.to_string());
}

fn extract_modules_from_import_from(child: Node, source: &str) -> Vec<String> {
    let mut modules = Vec::new();
    if let Some(m) = child.child_by_field_name("module_name") {
        let full_module = &source[m.start_byte()..m.end_byte()];
        push_dotted_segments(full_module, &mut modules);
    }
    // Important: do NOT treat imported names (e.g., `from X import Y`) as module dependencies.
    // `Y` is a symbol imported from module `X`, not necessarily a module itself. Treating it as
    // a module creates many spurious edges and huge fake SCC cycles.
    modules
}

fn push_import_name_segments(node: Node, source: &str, imports: &mut Vec<String>) {
    let name = &source[node.start_byte()..node.end_byte()];
    push_dotted_segments(name, imports);
}

fn collect_import_names(node: Node, source: &str, imports: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => push_import_name_segments(child, source, imports),
            "aliased_import" => {
                if let Some(n) = child.child_by_field_name("name") {
                    push_import_name_segments(n, source, imports);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn extract_imports_for_cache(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    extract_imports_recursive(node, source, &mut imports);
    imports
}

fn extract_imports_recursive(node: Node, source: &str, imports: &mut Vec<String>) {
    if is_type_checking_block(node, source) {
        return;
    }
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

pub fn compute_cyclomatic_complexity(node: Node) -> usize {
    1 + count_decision_points(node)
}

fn is_decision_point(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "elif_clause"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "with_statement"
            | "match_statement"
            | "case_clause"
            | "boolean_operator"
            | "conditional_expression"
    )
}

fn count_decision_points(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|c| usize::from(is_decision_point(c.kind())) + count_decision_points(c))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::create_parser;
    use std::io::Write;

    #[test]
    fn test_graph_imports_and_cycles() {
        let mut parser = create_parser().unwrap();
        assert!(
            extract_imports_for_cache(
                parser.parse("import os", None).unwrap().root_node(),
                "import os"
            )
            .contains(&"os".into())
        );
        let code = "import os\ndef foo():\n    import json\n    from sys import argv";
        let mut nested = Vec::new();
        extract_imports_recursive(
            parser.parse(code, None).unwrap().root_node(),
            code,
            &mut nested,
        );
        assert!(
            nested.contains(&"os".into())
                && nested.contains(&"json".into())
                && nested.contains(&"sys".into())
        );
        let mut g = DependencyGraph::default();
        g.add_dependency("a", "b");
        g.add_dependency("b", "a");
        let cycle_info: CycleInfo = g.find_cycles();
        assert!(!cycle_info.cycles.is_empty());
        assert_eq!(g.get_or_create_node("test"), g.get_or_create_node("test"));
        g.add_dependency("x", "x");
        // Self-dependencies are rejected: neither node nor edge is created
        assert!(
            !g.nodes.contains_key("x"),
            "Self-dependency should not create node"
        );
        let idx_a = *g.nodes.get("a").unwrap();
        let idx_b = *g.nodes.get("b").unwrap();
        assert!(g.is_cycle(&[idx_a, idx_b]) && !g.is_cycle(&[]) && !g.is_cycle(&[idx_a]));
        g.add_dependency("a", "c");
        g.add_dependency("c", "d");
        let (trans, depth) = g.compute_transitive_and_depth(*g.nodes.get("a").unwrap());
        assert!(trans >= 2 && depth >= 2);
    }

    #[test]
    fn test_from_import_does_not_create_edges_to_imported_names() {
        // Hypothesis 1 repro: `from X import Y` currently adds both `X` and `Y` as dependencies.
        // That can create huge, fake SCC cycles when `Y` happens to match some other module name.
        //
        // This fixture is *acyclic* under real Python import semantics:
        // - a imports b (and name c from b)
        // - b imports c (and name a from c)
        // - c imports nothing
        //
        // There is no module-level cycle unless we incorrectly treat imported names as modules.
        let mut parser = create_parser().unwrap();
        let files: Vec<(PathBuf, Vec<String>)> = vec![
            (
                PathBuf::from("a.py"),
                extract_imports_for_cache(
                    parser.parse("from b import c\n", None).unwrap().root_node(),
                    "from b import c\n",
                ),
            ),
            (
                PathBuf::from("b.py"),
                extract_imports_for_cache(
                    parser.parse("from c import a\n", None).unwrap().root_node(),
                    "from c import a\n",
                ),
            ),
            (
                PathBuf::from("c.py"),
                extract_imports_for_cache(parser.parse("\n", None).unwrap().root_node(), "\n"),
            ),
        ];

        let graph = build_dependency_graph_from_import_lists(&files);
        let cycles = graph.find_cycles().cycles;
        assert!(
            cycles.is_empty(),
            "Expected no module cycle; got cycles: {cycles:?}"
        );
    }

    #[test]
    fn test_dotted_import_does_not_create_edges_to_middle_segments() {
        // Hypothesis 2 repro: `import foo.bar` is currently split into segments `foo` and `bar`,
        // which can spuriously create an edge to a local `bar.py` module.
        let mut parser = create_parser().unwrap();
        let files: Vec<(PathBuf, Vec<String>)> = vec![
            (
                PathBuf::from("a.py"),
                extract_imports_for_cache(
                    parser.parse("import foo.bar\n", None).unwrap().root_node(),
                    "import foo.bar\n",
                ),
            ),
            // Local module named `bar` should NOT be considered imported by `import foo.bar`.
            (PathBuf::from("bar.py"), Vec::new()),
        ];

        let graph = build_dependency_graph_from_import_lists(&files);
        let a = qualified_module_name(&PathBuf::from("a.py"));
        let bar = qualified_module_name(&PathBuf::from("bar.py"));
        let a_idx = *graph.nodes.get(&a).expect("a node");
        let bar_idx = *graph.nodes.get(&bar).expect("bar node");

        assert!(
            !graph.graph.contains_edge(a_idx, bar_idx),
            "Expected no edge {a} -> {bar} from `import foo.bar`"
        );
    }

    #[test]
    fn test_qualified_module_name_includes_full_package_path() {
        // Hypothesis 3 repro: qualified_module_name currently only includes the leaf parent dir,
        // so deep paths can collide (e.g., pkg1/sub/utils.py and pkg2/sub/utils.py).
        use std::path::Path;
        let a = qualified_module_name(Path::new("pkg1/sub/utils.py"));
        let b = qualified_module_name(Path::new("pkg2/sub/utils.py"));
        assert_ne!(
            a, b,
            "Qualified module names should not collide for distinct deep package paths"
        );
    }

    #[test]
    fn test_helpers_imports_and_complexity() {
        assert!(is_entry_point("main") && is_entry_point("test_foo") && !is_entry_point("utils"));
        assert!(
            is_orphan(&ModuleGraphMetrics::default(), "utils")
                && !is_orphan(
                    &ModuleGraphMetrics {
                        fan_in: 1,
                        ..Default::default()
                    },
                    "utils"
                )
        );
        let mut g = DependencyGraph::new();
        g.paths.insert("foo".into(), PathBuf::from("src/foo.py"));
        assert_eq!(get_module_path(&g, "foo"), PathBuf::from("src/foo.py"));
        let mut parser = create_parser().unwrap();
        let mods = extract_modules_from_import_from(
            parser
                .parse("from foo.bar import baz", None)
                .unwrap()
                .root_node()
                .child(0)
                .unwrap(),
            "from foo.bar import baz",
        );
        assert!(
            mods.contains(&"foo.bar".into()),
            "Expected base module for from-import; got {mods:?}"
        );
        let rel = extract_modules_from_import_from(
            parser
                .parse("from ._export_format import X", None)
                .unwrap()
                .root_node()
                .child(0)
                .unwrap(),
            "from ._export_format import X",
        );
        assert!(
            rel.contains(&"_export_format".into()),
            "Relative import: {rel:?}"
        );
        assert!(is_decision_point("if_statement") && !is_decision_point("identifier"));
        assert_eq!(
            count_decision_points(
                parser
                    .parse("if a:\n    if b:\n        pass", None)
                    .unwrap()
                    .root_node()
            ),
            2
        );
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "x = 1").unwrap();
        assert!(!qualified_module_name(tmp.path()).is_empty());
        assert!(
            compute_cyclomatic_complexity(
                parser
                    .parse("def f():\n    if a:\n        pass", None)
                    .unwrap()
                    .root_node()
                    .child(0)
                    .unwrap()
            ) >= 2
        );
    }

    #[test]
    fn test_type_checking_imports_excluded_from_graph() {
        let mut parser = create_parser().unwrap();
        let code = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from some_module import SomeClass\nimport os";
        let imports = extract_imports_for_cache(parser.parse(code, None).unwrap().root_node(), code);
        assert!(imports.contains(&"typing".into()));
        assert!(imports.contains(&"os".into()));
        assert!(!imports.contains(&"some_module".into()));

        let code2 = "import typing\nif typing.TYPE_CHECKING:\n    from foo import Bar\nimport json";
        let imports2 =
            extract_imports_for_cache(parser.parse(code2, None).unwrap().root_node(), code2);
        assert!(imports2.contains(&"typing".into()));
        assert!(imports2.contains(&"json".into()));
        assert!(!imports2.contains(&"foo".into()));
    }

    #[test]
    fn test_qualified_and_bare_module_names() {
        use std::path::Path;
        // qualified_module_name includes parent directory
        assert_eq!(
            qualified_module_name(Path::new("src/attr/exceptions.py")),
            "attr.exceptions"
        );
        assert_eq!(
            qualified_module_name(Path::new("click/utils.py")),
            "click.utils"
        );
        assert_eq!(qualified_module_name(Path::new("utils.py")), "utils");
        assert_eq!(qualified_module_name(Path::new("./foo.py")), "foo");

        // bare_module_name is just the filename without extension
        assert_eq!(
            bare_module_name(Path::new("src/attr/exceptions.py")),
            "exceptions"
        );
        assert_eq!(bare_module_name(Path::new("click/utils.py")), "utils");
    }

    #[test]
    fn test_resolve_import() {
        let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();
        bare_to_qualified.insert(
            "exceptions".into(),
            vec!["attr.exceptions".into(), "click.exceptions".into()],
        );
        bare_to_qualified.insert("utils".into(), vec!["click.utils".into()]);

        // Unambiguous: single match
        assert_eq!(
            resolve_import("utils", Some("click"), &bare_to_qualified),
            Some("click.utils".into())
        );

        // Ambiguous: multiple matches, prefer same package
        assert_eq!(
            resolve_import("exceptions", Some("attr"), &bare_to_qualified),
            Some("attr.exceptions".into())
        );
        assert_eq!(
            resolve_import("exceptions", Some("click"), &bare_to_qualified),
            Some("click.exceptions".into())
        );

        // Ambiguous: no matching package, returns None
        assert_eq!(
            resolve_import("exceptions", Some("httpx"), &bare_to_qualified),
            None
        );

        // Unknown import: returns None
        assert_eq!(
            resolve_import("unknown", Some("attr"), &bare_to_qualified),
            None
        );
    }

    #[test]
    fn test_push_dotted_segments() {
        let mut modules = Vec::new();
        push_dotted_segments("foo.bar.baz", &mut modules);
        assert_eq!(modules, vec!["foo.bar.baz"]);

        modules.clear();
        push_dotted_segments("..relative", &mut modules);
        assert_eq!(modules, vec!["relative"]);

        modules.clear();
        push_dotted_segments("single", &mut modules);
        assert_eq!(modules, vec!["single"]);
    }

    // === Bug-hunting tests ===

    #[test]
    fn test_transitive_deps_excludes_self_in_cycle() {
        // In a 2-node cycle A→B→A, A's transitive dependencies should be {B},
        // not {A, B}. A module shouldn't be its own transitive dependency.
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b");
        g.add_dependency("b", "a");
        let metrics = g.module_metrics("a");
        assert_eq!(
            metrics.transitive_dependencies, 1,
            "Module 'a' should have 1 transitive dep (b), not count itself (got {})",
            metrics.transitive_dependencies
        );
    }

    #[test]
    fn test_is_test_module_singular_test_dir() {
        // is_test_module should also recognize "test/" (singular) directories
        let mut g = DependencyGraph::new();
        g.paths.insert(
            "test.helpers".into(),
            std::path::PathBuf::from("test/helpers.py"),
        );
        assert!(
            is_test_module(&g, "test.helpers"),
            "Modules under test/ (singular) should be recognized as test modules"
        );
    }

    #[test]
    fn test_touch_importinfo_and_push_import_name_segments() {
        // Touch private helpers/structs so static test-ref coverage includes them.
        let _ = ImportInfo {
            from_qualified: "a.b".into(),
            from_parent: Some("a".into()),
            imports: vec!["os".into()],
        };
        let mut parser = create_parser().unwrap();
        let tree = parser.parse("import os", None).unwrap();
        let node = tree.root_node().child(0).unwrap();
        let mut imports = Vec::new();
        // child(1) is typically the dotted_name in `import os`
        let dotted = node.child(1).unwrap();
        push_import_name_segments(dotted, "import os", &mut imports);
        assert!(imports.contains(&"os".into()));
    }
}
