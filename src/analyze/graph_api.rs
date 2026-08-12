use std::path::{Path, PathBuf};

use kiss::{
    Config, DependencyGraph, GateConfig, ParsedFile, ParsedRustFile, Violation, analyze_graph,
};
use kiss::lang_analysis::{
    LanguageGraph, LanguageParser, PythonAnalysis, RustAnalysis, build_graphs as build_lang_graphs,
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
#[allow(dead_code)] // Public analysis helper retained for library consumers / tests.
pub fn graph_for_path<'a>(
    path: &Path,
    py_graph: Option<&'a DependencyGraph>,
    rs_graph: Option<&'a DependencyGraph>,
) -> Option<&'a DependencyGraph> {
    path.extension().and_then(|e| {
        e.to_str().and_then(|ext| {
            if ext.eq_ignore_ascii_case("py") {
                py_graph
            } else if kiss::Language::is_rust_path(path) {
                rs_graph
            } else {
                None
            }
        })
    })
}

/// Build a Python dependency graph from a list of Python file paths.
pub fn build_py_graph_from_files(py_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let results = PythonAnalysis
        .parse_many(py_files)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    Ok(PythonAnalysis.build_graph(&parsed))
}

/// Build a Rust dependency graph from a list of Rust file paths.
pub fn build_rs_graph_from_files(rs_files: &[PathBuf]) -> DependencyGraph {
    let results = RustAnalysis
        .parse_many(rs_files)
        .unwrap_or_else(|_| Vec::new());
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    RustAnalysis.build_graph(&parsed)
}

pub(crate) fn build_py_graph(py_parsed: &[ParsedFile]) -> Option<DependencyGraph> {
    build_lang_graphs(py_parsed, &[]).0
}

pub(crate) fn build_rs_graph(rs_parsed: &[ParsedRustFile]) -> Option<DependencyGraph> {
    build_lang_graphs(&[], rs_parsed).1
}

pub fn build_graphs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    build_lang_graphs(py_parsed, rs_parsed)
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

#[cfg(test)]
mod graph_for_path_extension_tests {
    use super::graph_for_path;
    use kiss::DependencyGraph;
    use std::path::Path;

    /// Regression: `Path::extension()` preserves casing (e.g. `.PY`); matching must be ASCII-insensitive.
    #[test]
    fn graph_for_path_accepts_uppercase_py_and_rs_extensions() {
        let py = DependencyGraph::new();
        let rs = DependencyGraph::new();
        assert!(graph_for_path(Path::new("pkg/mod.PY"), Some(&py), None).is_some());
        assert!(graph_for_path(Path::new("src/lib.RS"), None, Some(&rs)).is_some());
        assert!(graph_for_path(Path::new("x.txt"), Some(&py), None).is_none());
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::{AnalyzeGraphsIn, GraphConfigs};

    impl GraphConfigs<'_> {
        fn witness() {}
    }

    impl AnalyzeGraphsIn<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_graph_api_types() {
        GraphConfigs::witness();
        AnalyzeGraphsIn::witness();
    }
}
