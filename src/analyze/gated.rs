#![allow(dead_code)]

use kiss::{GateConfig, ParsedFile, ParsedRustFile};

use crate::analyze::coverage_gate::evaluate_gate;
use crate::analyze::finalize::{AnalysisProducts, FinalizeAnalysisIn, finalize_analysis};
use crate::analyze::graph_api::{build_py_graph, build_rs_graph};
use crate::analyze::options::AnalyzeResult;
use crate::analyze::parallel::{BuildGraphViols, RustAnalysis, build_graph_violations};
use crate::analyze::params::GatedAnalysis;
use std::path::{Path, PathBuf};

type PyDup = kiss::DuplicateCluster;

fn repo_root_for_universe(universe: &str) -> PathBuf {
    Path::new(universe)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(universe))
}

struct GatedPyParallelIn<'a> {
    py_parsed: &'a [ParsedFile],
    opts: &'a crate::analyze::options::AnalyzeOptions<'a>,
    file_count: usize,
    gate: &'a GateConfig,
}

fn gated_py_parallel(
    in_: &GatedPyParallelIn<'_>,
) -> (
    kiss::TestRefAnalysis,
    Option<kiss::DependencyGraph>,
    Vec<kiss::Violation>,
    Vec<PyDup>,
) {
    use crate::analyze::dup_detect;

    let GatedPyParallelIn {
        py_parsed,
        opts,
        file_count,
        gate,
    } = in_;
    let orphan_enabled = gate.orphan_module_enabled;
    let dup_enabled = gate.duplication_enabled;
    let min_sim = gate.min_similarity;

    let (py_cov, (py_graph, graph_viols_all, py_dups_all)) = rayon::join(
        || {
            let repo_root = Path::new(opts.universe)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(opts.universe));
            kiss::rslip_bridge::runtime_py_analysis(&repo_root, py_parsed, opts.jobs)
        },
        || {
            let py_graph = build_py_graph(py_parsed);
            let (gv, py_dups) = rayon::join(
                || {
                    build_graph_violations(BuildGraphViols {
                        py_graph: py_graph.as_ref(),
                        rs_graph: None,
                        py_config: opts.py_config,
                        rs_config: opts.rs_config,
                        file_count: *file_count,
                        orphan_enabled,
                    })
                },
                || {
                    if dup_enabled {
                        dup_detect::detect_py_duplicates(py_parsed, min_sim)
                    } else {
                        Vec::new()
                    }
                },
            );
            (py_graph, gv, py_dups)
        },
    );
    (py_cov, py_graph, graph_viols_all, py_dups_all)
}

fn gated_runtime_rust_coverage_for_opts(
    opts: &crate::analyze::options::AnalyzeOptions<'_>,
    parsed: &[ParsedRustFile],
) -> kiss::RustTestRefAnalysis {
    let repo_root = repo_root_for_universe(opts.universe);
    kiss::rust_llvm_cov::runtime_rust_analysis(&repo_root, parsed)
}

pub(crate) fn run_gated_analysis(in_: GatedAnalysis<'_>) -> AnalyzeResult {
    use crate::analyze::dup_detect;

    let GatedAnalysis {
        opts,
        py_files,
        rs_files,
        focus,
        parsed: (result, viols, file_count),
        timings,
    } = in_;
    let rs_cov = gated_runtime_rust_coverage_for_opts(opts, &result.rs_parsed);

    let (py_cov, py_graph, mut graph_viols_all, py_dups_all) =
        gated_py_parallel(&GatedPyParallelIn {
            py_parsed: &result.py_parsed,
            opts,
            file_count,
            gate: opts.gate_config,
        });

    if let Some(early) = evaluate_gate(
        &py_cov,
        &rs_cov,
        &result.py_parsed,
        &result.rs_parsed,
        focus,
        opts.gate_config.test_coverage_threshold,
    ) {
        return early;
    }

    let rs_graph = build_rs_graph(&result.rs_parsed);
    if let Some(ref g) = rs_graph {
        graph_viols_all.extend(kiss::analyze_graph(
            g,
            opts.rs_config,
            opts.gate_config.orphan_module_enabled,
        ));
    }
    let rs = RustAnalysis {
        graph: rs_graph,
        cov: rs_cov,
        dups: if opts.gate_config.duplication_enabled {
            dup_detect::detect_rs_duplicates(&result.rs_parsed, opts.gate_config.min_similarity)
        } else {
            Vec::new()
        },
    };

    finalize_analysis(FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus,
        products: AnalysisProducts {
            result,
            viols,
            file_count,
            py_cov,
            cov_viols: Vec::new(),
            coverage_cache_lists: None,
            py_stats: None,
            rs_stats: None,
            rs,
            py_graph,
            graph_viols_all,
            py_dups_all,
        },
        timings,
    })
}

#[cfg(test)]
#[path = "gated_test.rs"]
mod gated_tests;
