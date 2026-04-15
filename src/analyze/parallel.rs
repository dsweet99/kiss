use kiss::{
    Config, DependencyGraph, DuplicateCluster, GateConfig, ParsedFile, ParsedRustFile, Violation,
    analyze_graph,
};

use crate::analyze::dup_detect::{detect_py_duplicates, detect_rs_duplicates};
use crate::analyze::graph_api::{build_py_graph, build_rs_graph};
use crate::analyze::options::AnalyzeOptions;

pub(crate) struct RustAnalysis {
    pub graph: Option<DependencyGraph>,
    pub cov: kiss::RustTestRefAnalysis,
    pub dups: Vec<DuplicateCluster>,
}

pub(crate) fn run_rust_analysis(
    rs_parsed: &[ParsedRustFile],
    gate_config: &GateConfig,
    cached_rs_cov: Option<kiss::RustTestRefAnalysis>,
) -> RustAnalysis {
    let graph = build_rs_graph(rs_parsed);
    let cov = cached_rs_cov.unwrap_or_else(|| {
        let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        kiss::analyze_rust_test_refs(&rs_refs, graph.as_ref())
    });
    let dups = if gate_config.duplication_enabled {
        detect_rs_duplicates(rs_parsed, gate_config.min_similarity)
    } else {
        Vec::new()
    };
    RustAnalysis { graph, cov, dups }
}

type GraphResult = (Option<DependencyGraph>, Vec<Violation>);
type CoverageResult = (kiss::TestRefAnalysis, Vec<DuplicateCluster>);

/// Parallel Python graph/coverage + duplication work.
pub(crate) struct ParallelPyIn<'a> {
    pub py_parsed: &'a [ParsedFile],
    pub rs_graph: Option<&'a DependencyGraph>,
    pub opts: &'a AnalyzeOptions<'a>,
    pub file_count: usize,
    pub cached_py_cov: Option<kiss::TestRefAnalysis>,
}

pub(crate) fn run_parallel_py_analysis(in_: ParallelPyIn<'_>) -> (GraphResult, CoverageResult) {
    let ParallelPyIn {
        py_parsed,
        rs_graph,
        opts,
        file_count,
        cached_py_cov,
    } = in_;
    let orphan_enabled = opts.gate_config.orphan_module_enabled;
    let dup_enabled = opts.gate_config.duplication_enabled;
    let min_sim = opts.gate_config.min_similarity;
    let py_graph = build_py_graph(py_parsed);
    let (gv, (py_cov, py_dups)) = rayon::join(
        || {
            build_graph_violations(BuildGraphViols {
                py_graph: py_graph.as_ref(),
                rs_graph,
                py_config: opts.py_config,
                rs_config: opts.rs_config,
                file_count,
                orphan_enabled,
            })
        },
        || {
            let py_cov = cached_py_cov.unwrap_or_else(|| {
                let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
                kiss::analyze_test_refs_no_map(&py_refs, py_graph.as_ref())
            });
            let py_dups = if dup_enabled {
                detect_py_duplicates(py_parsed, min_sim)
            } else {
                Vec::new()
            };
            (py_cov, py_dups)
        },
    );
    ((py_graph, gv), (py_cov, py_dups))
}

pub(crate) struct BuildGraphViols<'a> {
    pub py_graph: Option<&'a DependencyGraph>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub file_count: usize,
    pub orphan_enabled: bool,
}

pub(crate) fn build_graph_violations(in_: BuildGraphViols<'_>) -> Vec<Violation> {
    let BuildGraphViols {
        py_graph,
        rs_graph,
        py_config,
        rs_config,
        file_count,
        orphan_enabled,
    } = in_;
    if file_count <= 1 {
        return Vec::new();
    }
    let mut gv = Vec::new();
    if let Some(g) = py_graph {
        gv.extend(analyze_graph(g, py_config, orphan_enabled));
    }
    if let Some(g) = rs_graph {
        gv.extend(analyze_graph(g, rs_config, orphan_enabled));
    }
    gv
}

#[cfg(test)]
mod parallel_touch {
    use super::{BuildGraphViols, ParallelPyIn};

    #[test]
    fn struct_sizes_for_gate() {
        let _ = std::mem::size_of::<ParallelPyIn>();
        let _ = std::mem::size_of::<BuildGraphViols>();
    }
}
