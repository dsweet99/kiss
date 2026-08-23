use crate::code_roles::FileComposition;

pub struct DependencyGraph {
    pub graph: DiGraph<String, ()>,
    pub nodes: HashMap<String, NodeIndex>,
    pub paths: HashMap<String, PathBuf>,
    pub path_to_module: HashMap<PathBuf, String>,
    pub compositions: HashMap<String, FileComposition>,
}

#[derive(Debug, Default)]
pub struct ModuleGraphMetrics {
    pub fan_in: usize,
    pub fan_out: usize,
    pub indirect_dependencies: usize,
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
            path_to_module: HashMap::new(),
            compositions: HashMap::new(),
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
        let mut stamp = vec![0u32; self.graph.node_count()];
        let mut stamp_gen = 0u32;
        metrics_at(self, idx, &mut stamp, &mut stamp_gen)
    }

    pub(crate) fn is_cycle(&self, scc: &[NodeIndex]) -> bool {
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

    pub fn module_for_path(&self, path: &std::path::Path) -> Option<String> {
        self.path_to_module.get(path).cloned()
    }

    #[cfg(test)]
    pub fn test_importers_of(&self, module: &str) -> Vec<String> {
        let Some(&idx) = self.nodes.get(module) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .map(|i| self.graph[i].clone())
            .filter(|m| self.compositions.get(m) == Some(&FileComposition::TestOnly))
            .collect()
    }

    pub fn imports(&self, from_module: &str, to_module: &str) -> bool {
        let (Some(&from_idx), Some(&to_idx)) =
            (self.nodes.get(from_module), self.nodes.get(to_module))
        else {
            return false;
        };
        self.graph.find_edge(from_idx, to_idx).is_some()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn file_stem_str(path: &Path) -> &str {
    path.file_stem()
        .map_or("unknown", |s| s.to_str().unwrap_or("unknown"))
}
pub(crate) fn parent_dir_strings(path: &Path) -> Vec<String> {
    path.parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(os) => os.to_str().map(std::string::ToString::to_string),
                    Component::CurDir => Some(".".to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn trim_src_suffix(mut dirs: Vec<String>) -> Vec<String> {
    if let Some(pos) = dirs.iter().rposition(|d| d == "src") {
        dirs = dirs[(pos + 1)..].to_vec();
    }
    dirs.retain(|d| d != ".");
    dirs
}
pub(crate) fn join_qualified_dirs_and_stem(dirs: &[String], stem: &str) -> String {
    format!("{}.{}", dirs.join("."), stem)
}
pub fn qualified_module_name(path: &Path) -> String {
    let stem = file_stem_str(path);
    let dirs = trim_src_suffix(parent_dir_strings(path));
    if stem == "__init__" {
        return if dirs.is_empty() {
            stem.to_string()
        } else {
            dirs.join(".")
        };
    }
    if dirs.is_empty() {
        stem.to_string()
    } else {
        join_qualified_dirs_and_stem(&dirs, stem)
    }
}
pub(crate) fn bare_module_name(path: &Path) -> String {
    let stem = path.file_stem().map_or_else(
        || "unknown".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    if stem == "__init__" {
        path.parent()
            .and_then(|p| p.file_name())
            .map_or(stem, |p| p.to_string_lossy().into_owned())
    } else {
        stem
    }
}
pub fn is_entry_point(name: &str) -> bool {
    let bare = name.rsplit('.').next().unwrap_or(name);
    if name == "bin" || name.starts_with("bin.") || name.contains(".bin.") {
        return true;
    }
    matches!(
        bare,
        "main" | "lib" | "build" | "__main__" | "__init__" | "setup"
    )
}
fn metrics_at(
    graph: &DependencyGraph,
    idx: NodeIndex,
    stamp: &mut [u32],
    stamp_gen: &mut u32,
) -> ModuleGraphMetrics {
    let fan_out = graph
        .graph
        .neighbors_directed(idx, petgraph::Direction::Outgoing)
        .count();
    let (total_reachable, depth) = compute_reachable_and_depth(graph, idx, stamp, stamp_gen);
    ModuleGraphMetrics {
        fan_in: graph
            .graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .count(),
        fan_out,
        indirect_dependencies: total_reachable.saturating_sub(fan_out),
        dependency_depth: depth,
    }
}

fn compute_reachable_and_depth(
    graph: &DependencyGraph,
    start: NodeIndex,
    stamp: &mut [u32],
    stamp_gen: &mut u32,
) -> (usize, usize) {
    use std::collections::VecDeque;
    *stamp_gen = stamp_gen.wrapping_add(1);
    if *stamp_gen == 0 {
        stamp.fill(0);
        *stamp_gen = 1;
    }
    let mut queue = VecDeque::new();
    let mut max_depth = 0;
    let mut reached = 0;
    stamp[start.index()] = *stamp_gen;
    queue.push_back((start, 0));
    while let Some((node, depth)) = queue.pop_front() {
        for neighbor in graph
            .graph
            .neighbors_directed(node, petgraph::Direction::Outgoing)
        {
            let i = neighbor.index();
            if stamp[i] != *stamp_gen {
                stamp[i] = *stamp_gen;
                reached += 1;
                max_depth = max_depth.max(depth + 1);
                queue.push_back((neighbor, depth + 1));
            }
        }
    }
    (reached, max_depth)
}

pub fn all_module_metrics(graph: &DependencyGraph) -> HashMap<String, ModuleGraphMetrics> {
    let mut stamp = vec![0u32; graph.graph.node_count()];
    let mut stamp_gen = 0u32;
    let mut out = HashMap::with_capacity(graph.nodes.len());
    for (name, &idx) in &graph.nodes {
        out.insert(name.clone(), metrics_at(graph, idx, &mut stamp, &mut stamp_gen));
    }
    out
}

pub(crate) fn is_orphan(fan_in: usize, fan_out: usize, module_name: &str) -> bool {
    fan_in == 0 && fan_out == 0 && !is_entry_point(module_name)
}

pub(crate) fn is_crate_root_aggregator(graph: &DependencyGraph, module_name: &str) -> bool {
    let Some(path) = graph.paths.get(module_name) else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if !matches!(file_name, "lib.rs" | "main.rs" | "build.rs") {
        return false;
    }
    path.parent().is_some_and(|parent| {
        parent
            .file_name()
            .is_some_and(|d| d == OsStr::new("src") || d == OsStr::new("tests"))
    })
}
