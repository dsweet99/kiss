pub(crate) fn get_module_path(graph: &DependencyGraph, module_name: &str) -> PathBuf {
    graph
        .paths
        .get(module_name)
        .cloned()
        .unwrap_or_else(|| PathBuf::from(format!("{module_name}.py")))
}

pub(crate) fn is_init_module(graph: &DependencyGraph, module_name: &str) -> bool {
    graph
        .paths
        .get(module_name)
        .and_then(|p| p.file_stem())
        .is_some_and(|s| s == "__init__")
}

pub(crate) fn path_dedup_set(graph: &DependencyGraph) -> HashMap<PathBuf, Vec<String>> {
    let mut map: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for (name, path) in &graph.paths {
        map.entry(path.clone()).or_default().push(name.clone());
    }
    map
}

pub(crate) fn is_path_covered_by_another(
    graph: &DependencyGraph,
    module_name: &str,
    path_groups: &HashMap<PathBuf, Vec<String>>,
) -> bool {
    let Some(path) = graph.paths.get(module_name) else {
        return false;
    };
    let Some(siblings) = path_groups.get(path) else {
        return false;
    };
    siblings.len() > 1
        && siblings.iter().any(|sibling| {
            sibling != module_name && {
                let m = graph.module_metrics(sibling);
                m.fan_in > 0 || m.fan_out > 0
            }
        })
}

pub(crate) fn orphan_violation(graph: &DependencyGraph, module_name: &str) -> Violation {
    Violation {
        file: get_module_path(graph, module_name),
        line: 1,
        unit_name: module_name.to_string(),
        metric: "orphan_module".to_string(),
        value: 0,
        threshold: 0,
        message: format!(
            "Module '{module_name}' is an isolated production module (no production or test-only import edges)"
        ),
        suggestion: "This is not a recognized entry. Import it from production or test code, declare it as an entry (script/bin/__main__/fn main), or add its directory to orphan_allowed."
            .to_string(),
    }
}

pub(crate) fn indirect_deps_violation(
    graph: &DependencyGraph,
    module_name: &str,
    metrics: &ModuleGraphMetrics,
    threshold: usize,
) -> Violation {
    Violation {
        file: get_module_path(graph, module_name),
        line: 1,
        unit_name: module_name.to_string(),
        metric: "indirect_dependencies".to_string(),
        value: metrics.indirect_dependencies,
        threshold,
        message: format!(
            "Module '{}' has {} indirect dependencies (threshold: {})",
            module_name, metrics.indirect_dependencies, threshold
        ),
        suggestion:
            "Reduce coupling by introducing abstraction layers or splitting responsibilities."
                .to_string(),
    }
}

pub(crate) fn dependency_depth_violation(
    graph: &DependencyGraph,
    module_name: &str,
    metrics: &ModuleGraphMetrics,
    threshold: usize,
) -> Violation {
    Violation {
        file: get_module_path(graph, module_name),
        line: 1,
        unit_name: module_name.to_string(),
        metric: "dependency_depth".to_string(),
        value: metrics.dependency_depth,
        threshold,
        message: format!(
            "Module '{}' has dependency depth {} (threshold: {})",
            module_name, metrics.dependency_depth, threshold
        ),
        suggestion: "Flatten the dependency chain by moving shared logic to a common base layer."
            .to_string(),
    }
}

pub(crate) fn cycle_size_violation(
    graph: &DependencyGraph,
    cycle: &[String],
    threshold: usize,
) -> Violation {
    let cycle_str = cycle.join(" → ");
    let first_module = cycle.first().cloned().unwrap_or_default();
    Violation {
        file: get_module_path(graph, &first_module),
        line: 1,
        unit_name: first_module,
        metric: "cycle_size".to_string(),
        value: cycle.len(),
        threshold,
        message: format!(
            "Circular dependency detected ({} modules, threshold: {}): {} → {}",
            cycle.len(),
            threshold,
            cycle_str,
            cycle.first().unwrap_or(&String::new())
        ),
        suggestion: "Large cycles are harder to untangle. Prefer breaking an edge that reduces coupling to the rest of the graph, or split a module on the cycle behind a smaller interface."
            .to_string(),
    }
}

pub(crate) fn collect_module_violations(
    graph: &DependencyGraph,
    config: &Config,
    _seen_paths: &HashMap<PathBuf, Vec<String>>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let metrics_by_module = crate::graph::all_module_metrics(graph);
    for module_name in graph.nodes.keys() {
        if !graph.paths.contains_key(module_name) {
            continue;
        }
        let Some(metrics) = metrics_by_module.get(module_name) else {
            continue;
        };

        if metrics.indirect_dependencies > config.indirect_dependencies
            && !is_crate_root_aggregator(graph, module_name)
        {
            violations.push(indirect_deps_violation(
                graph,
                module_name,
                metrics,
                config.indirect_dependencies,
            ));
        }
        if metrics.dependency_depth > config.dependency_depth {
            violations.push(dependency_depth_violation(
                graph,
                module_name,
                metrics,
                config.dependency_depth,
            ));
        }
    }
    violations
}

#[must_use]
pub fn compute_cyclomatic_complexity(node: Node) -> usize {
    1 + count_decision_points(node)
}

pub(crate) fn is_decision_point(kind: &str) -> bool {
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

pub(crate) fn count_decision_points(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|c| usize::from(is_decision_point(c.kind())) + count_decision_points(c))
        .sum()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphKeyMaxima {
    pub indirect_dependencies: usize,
    pub dependency_depth: usize,
    pub cycle_size: usize,
}

#[must_use]
pub fn graph_key_maxima(graph: &DependencyGraph) -> GraphKeyMaxima {
    let mut indirect_dependencies = 0;
    let mut dependency_depth = 0;
    let metrics_by_module = crate::graph::all_module_metrics(graph);
    for module_name in graph.nodes.keys() {
        if !graph.paths.contains_key(module_name) {
            continue;
        }
        let Some(metrics) = metrics_by_module.get(module_name) else {
            continue;
        };
        if !is_crate_root_aggregator(graph, module_name) {
            indirect_dependencies = indirect_dependencies.max(metrics.indirect_dependencies);
        }
        dependency_depth = dependency_depth.max(metrics.dependency_depth);
    }
    let cycle_size = graph
        .find_cycles()
        .cycles
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    GraphKeyMaxima {
        indirect_dependencies,
        dependency_depth,
        cycle_size,
    }
}

#[must_use]
pub fn analyze_graph(
    graph: &DependencyGraph,
    config: &Config,
) -> Vec<Violation> {
    let seen_paths = path_dedup_set(graph);
    let mut violations =
        collect_module_violations(graph, config, &seen_paths);
    for cycle in graph.find_cycles().cycles {
        if cycle.len() > config.cycle_size {
            violations.push(cycle_size_violation(graph, &cycle, config.cycle_size));
        }
    }
    violations
}
