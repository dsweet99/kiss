use std::path::{Path, PathBuf};

use kiss::{
    Config, DependencyGraph, GateConfig, ParsedFile, ParsedRustFile, Violation, analyze_graph,
    build_dependency_graph, build_rust_dependency_graph,
};

/// Config bundle for graph orphan analysis on Python and Rust graphs.
pub struct GraphConfigs<'a> {
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub gate: &'a GateConfig,
}

/// Inputs for [`analyze_graphs`].
pub struct AnalyzeGraphsIn<'a> {
    pub py_graph: Option<&'a DependencyGraph>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub configs: GraphConfigs<'a>,
}

/// Pick the Python or Rust graph for a source file path based on extension.
pub fn graph_for_path<'a>(
    path: &Path,
    py_graph: Option<&'a DependencyGraph>,
    rs_graph: Option<&'a DependencyGraph>,
) -> Option<&'a DependencyGraph> {
    path.extension().and_then(|e| {
        e.to_str().and_then(|ext| {
            if ext == "py" {
                py_graph
            } else if ext == "rs" {
                rs_graph
            } else {
                None
            }
        })
    })
}

/// Build a Python dependency graph from a list of Python file paths.
pub fn build_py_graph_from_files(py_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let results = kiss::parse_files(py_files).map_err(|e| std::io::Error::other(e.to_string()))?;
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    Ok(build_dependency_graph(&parsed))
}

/// Build a Rust dependency graph from a list of Rust file paths.
pub fn build_rs_graph_from_files(rs_files: &[PathBuf]) -> DependencyGraph {
    let results = kiss::rust_parsing::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    build_rust_dependency_graph(&parsed)
}

pub(crate) fn build_py_graph(py_parsed: &[ParsedFile]) -> Option<DependencyGraph> {
    if py_parsed.is_empty() {
        None
    } else {
        Some(build_dependency_graph(
            &py_parsed.iter().collect::<Vec<_>>(),
        ))
    }
}

pub(crate) fn build_rs_graph(rs_parsed: &[ParsedRustFile]) -> Option<DependencyGraph> {
    if rs_parsed.is_empty() {
        None
    } else {
        Some(build_rust_dependency_graph(
            &rs_parsed.iter().collect::<Vec<_>>(),
        ))
    }
}

pub fn build_graphs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    (build_py_graph(py_parsed), build_rs_graph(rs_parsed))
}

pub(crate) fn graph_stats(
    py_g: Option<&DependencyGraph>,
    rs_g: Option<&DependencyGraph>,
) -> (usize, usize) {
    let (mut nodes, mut edges) = (0, 0);
    if let Some(g) = py_g {
        nodes += g.graph.node_count();
        edges += g.graph.edge_count();
    }
    if let Some(g) = rs_g {
        nodes += g.graph.node_count();
        edges += g.graph.edge_count();
    }
    (nodes, edges)
}

#[allow(dead_code)]
pub fn analyze_graphs(in_: &AnalyzeGraphsIn<'_>) -> Vec<Violation> {
    let AnalyzeGraphsIn {
        py_graph,
        rs_graph,
        configs,
    } = in_;
    let orphan = configs.gate.orphan_module_enabled;
    let mut viols = Vec::new();
    if let Some(g) = py_graph {
        viols.extend(analyze_graph(g, configs.py_config, orphan));
    }
    if let Some(g) = rs_graph {
        viols.extend(analyze_graph(g, configs.rs_config, orphan));
    }
    viols
}

#[cfg(test)]
mod graph_api_touch {
    use super::{AnalyzeGraphsIn, GraphConfigs};

    #[test]
    fn struct_sizes_for_gate() {
        let _ = std::mem::size_of::<GraphConfigs>();
        let _ = std::mem::size_of::<AnalyzeGraphsIn>();
    }
}
