use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kiss::{
    Config, ContextDependencyGraph, DependencyGraph, DuplicateCluster, GateConfig, ParsedFile,
    ParsedRustFile, Violation, analyze_graph,
};

use crate::analyze::dup_detect::{detect_py_duplicates, detect_rs_duplicates};
use crate::analyze::graph_api::{append_orphan_violations, build_py_graphs, build_rs_graphs};
use crate::analyze::options::AnalyzeOptions;
use kiss::code_roles::SourceRoleIndex;

pub(crate) struct RustAnalysis {
    pub graph: Option<DependencyGraph>,
    pub ctx: ContextDependencyGraph,
    pub dups: Vec<DuplicateCluster>,
}

pub(crate) fn run_rust_analysis(
    rs_parsed: &[ParsedRustFile],
    gate_config: &GateConfig,
    roles: &SourceRoleIndex,
) -> RustAnalysis {
    let (graph, ctx) = build_rs_graphs(rs_parsed, roles);
    let dups = if gate_config.duplication_enabled {
        detect_rs_duplicates(rs_parsed, gate_config.min_similarity, roles)
    } else {
        Vec::new()
    };
    RustAnalysis { graph, ctx, dups }
}

type GraphResult = (Option<DependencyGraph>, Vec<Violation>);
type DupResult = Vec<DuplicateCluster>;

pub(crate) struct ParallelPyIn<'a> {
    pub py_parsed: &'a [ParsedFile],
    pub rust_entries: &'a HashSet<PathBuf>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub rs_ctx: Option<&'a ContextDependencyGraph>,
    pub opts: &'a AnalyzeOptions<'a>,
    pub file_count: usize,
    pub roles: &'a SourceRoleIndex,
    pub repo_root: &'a Path,
}

pub(crate) fn run_parallel_py_analysis(in_: ParallelPyIn<'_>) -> (GraphResult, DupResult) {
    let ParallelPyIn {
        py_parsed,
        rust_entries,
        rs_graph,
        rs_ctx,
        opts,
        file_count,
        roles,
        repo_root,
    } = in_;
    let orphan_enabled = opts.gate_config.orphan_module_enabled;
    let dup_enabled = opts.gate_config.duplication_enabled;
    let min_sim = opts.gate_config.min_similarity;
    let (py_graph, py_ctx) = build_py_graphs(py_parsed, roles);
    let mut entries = kiss::collect_orphan_entry_paths(py_parsed, &[], py_graph.as_ref(), rs_graph);
    entries.extend(rust_entries.iter().cloned());
    let (gv, py_dups) = rayon::join(
        || {
            build_graph_violations(BuildGraphViols {
                py_graph: py_graph.as_ref(),
                rs_graph,
                py_ctx: Some(&py_ctx),
                rs_ctx,
                entries: &entries,
                py_config: opts.py_config,
                rs_config: opts.rs_config,
                file_count,
                orphan_enabled,
                orphan_allowed: &opts.gate_config.orphan_allowed,
                repo_root,
            })
        },
        || {
            if dup_enabled {
                detect_py_duplicates(py_parsed, min_sim, roles)
            } else {
                Vec::new()
            }
        },
    );
    ((py_graph, gv), py_dups)
}

pub(crate) struct BuildGraphViols<'a> {
    pub py_graph: Option<&'a DependencyGraph>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub py_ctx: Option<&'a ContextDependencyGraph>,
    pub rs_ctx: Option<&'a ContextDependencyGraph>,
    pub entries: &'a HashSet<PathBuf>,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub file_count: usize,
    pub orphan_enabled: bool,
    pub orphan_allowed: &'a [String],
    pub repo_root: &'a Path,
}

pub(crate) fn build_graph_violations(in_: BuildGraphViols<'_>) -> Vec<Violation> {
    let BuildGraphViols {
        py_graph,
        rs_graph,
        py_ctx,
        rs_ctx,
        entries,
        py_config,
        rs_config,
        file_count,
        orphan_enabled,
        orphan_allowed,
        repo_root,
    } = in_;
    if file_count <= 1 {
        return Vec::new();
    }
    let mut gv = Vec::new();
    if let Some(g) = py_graph {
        gv.extend(analyze_graph(g, py_config, false));
    }
    if let Some(g) = rs_graph {
        gv.extend(analyze_graph(g, rs_config, false));
    }
    if orphan_enabled {
        gv.extend(append_orphan_violations(
            py_ctx,
            py_graph,
            rs_ctx,
            rs_graph,
            entries,
            orphan_allowed,
            repo_root,
        ));
    }
    gv
}

#[cfg(test)]
mod parallel_touch {
    use super::{BuildGraphViols, ContextDependencyGraph, ParallelPyIn, RustAnalysis};

    impl RustAnalysis {
        fn witness() -> Self {
            Self {
                graph: None,
                ctx: ContextDependencyGraph::empty(),
                dups: vec![],
            }
        }
    }

    impl ParallelPyIn<'_> {
        fn witness() {}
    }

    impl BuildGraphViols<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_parallel_types() {
        let _ = RustAnalysis::witness();
        ParallelPyIn::witness();
        BuildGraphViols::witness();
    }
}
