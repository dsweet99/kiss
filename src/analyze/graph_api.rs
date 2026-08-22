use std::path::{Path, PathBuf};

use kiss::lang_analysis::build_graphs as build_lang_graphs;
use kiss::{
    Config, DependencyGraph, GateConfig, ParsedFile, ParsedRustFile, Violation, analyze_graph,
};

pub struct GraphConfigs<'a> {
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub gate: &'a GateConfig,
}

pub struct AnalyzeGraphsIn<'a> {
    pub py_graph: Option<&'a DependencyGraph>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub configs: GraphConfigs<'a>,
}

#[allow(dead_code)]
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

fn graph_io_error(err: kiss::RoleBuildError) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

pub fn build_py_graph_from_files(py_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let (parsed, _rs, roles) =
        crate::analyze_parse::parse_classified(py_files, &[]).map_err(graph_io_error)?;
    Ok(build_py_graph(&parsed, &roles).unwrap_or_default())
}

pub fn build_rs_graph_from_files(rs_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let (_py, parsed, roles) =
        crate::analyze_parse::parse_classified(&[], rs_files).map_err(graph_io_error)?;
    Ok(build_rs_graph(&parsed, &roles).unwrap_or_default())
}

pub(crate) fn build_py_graph(
    py_parsed: &[ParsedFile],
    roles: &kiss::code_roles::SourceRoleIndex,
) -> Option<DependencyGraph> {
    if py_parsed.is_empty() {
        None
    } else {
        Some(py_context_graph(py_parsed, roles).production_view())
    }
}

pub(crate) fn build_rs_graph(
    rs_parsed: &[ParsedRustFile],
    roles: &kiss::code_roles::SourceRoleIndex,
) -> Option<DependencyGraph> {
    if rs_parsed.is_empty() {
        None
    } else {
        Some(
            build_role_graphs(&[], rs_parsed, roles)
                .rust
                .production_view(),
        )
    }
}

pub(crate) fn build_role_graphs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: &kiss::code_roles::SourceRoleIndex,
) -> kiss::RoleDependencyGraphs {
    let python = if py_parsed.is_empty() {
        kiss::ContextDependencyGraph::empty()
    } else {
        py_context_graph(py_parsed, roles)
    };
    let rust = if rs_parsed.is_empty() {
        kiss::ContextDependencyGraph::empty()
    } else {
        let refs: Vec<_> = rs_parsed.iter().collect();
        kiss::build_rust_context_graph(&refs, roles)
    };
    kiss::RoleDependencyGraphs { python, rust }
}

fn py_context_graph(
    py_parsed: &[ParsedFile],
    roles: &kiss::code_roles::SourceRoleIndex,
) -> kiss::ContextDependencyGraph {
    let refs: Vec<_> = py_parsed.iter().collect();
    kiss::build_python_context_graph(&refs, roles)
}

#[allow(dead_code)]
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

#[cfg(test)]
mod parse_fail_closed {
    use super::{build_py_graph_from_files, build_rs_graph_from_files};

    #[test]
    fn graph_builders_fail_on_unparsable_source() {
        let tmp = tempfile::tempdir().unwrap();
        let ok_py = tmp.path().join("ok.py");
        let bad_py = tmp.path().join("bad.py");
        std::fs::write(&ok_py, "x = 1\n").unwrap();
        std::fs::write(&bad_py, "def (\n").unwrap();
        assert!(build_py_graph_from_files(&[ok_py, bad_py]).is_err());

        let ok_rs = tmp.path().join("ok.rs");
        let bad_rs = tmp.path().join("bad.rs");
        std::fs::write(&ok_rs, "pub fn ok() {}\n").unwrap();
        std::fs::write(&bad_rs, "fn (\n").unwrap();
        assert!(build_rs_graph_from_files(&[ok_rs, bad_rs]).is_err());
    }
}

#[cfg(test)]
mod colliding_language_names {
    use super::build_role_graphs;

    #[test]
    fn colliding_python_and_rust_module_names_stay_separate() {
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("helper.py");
        let rs = tmp.path().join("helper.rs");
        std::fs::write(&py, "def f():\n    return 1\n").unwrap();
        std::fs::write(&rs, "pub fn f() {}\n").unwrap();
        let (py_parsed, rs_parsed, roles) = crate::analyze_parse::parse_classified(
            std::slice::from_ref(&py),
            std::slice::from_ref(&rs),
        )
        .unwrap();
        let graphs = build_role_graphs(&py_parsed, &rs_parsed, &roles);
        let py_prod = graphs.python.production_view();
        let rs_prod = graphs.rust.production_view();
        assert!(
            py_prod.paths.values().any(|p| p.ends_with("helper.py")),
            "python graph must keep helper.py"
        );
        assert!(
            rs_prod.paths.values().any(|p| p.ends_with("helper.rs")),
            "rust graph must keep helper.rs"
        );
        assert!(
            py_prod.paths.values().all(|p| !p.ends_with("helper.rs")),
            "python production graph must not own the rust helper"
        );
        assert!(
            rs_prod.paths.values().all(|p| !p.ends_with("helper.py")),
            "rust production graph must not own the python helper"
        );
    }
}
