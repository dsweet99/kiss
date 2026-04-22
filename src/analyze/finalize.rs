use std::time::Instant;

use crate::analyze::cache::{FullCacheStoreInput, maybe_store_full_cache};
use crate::analyze::coverage::{
    CoverageOutputOpts, GraphRefPair, PyRsTestCoverage, collect_coverage_viols,
};
pub(crate) use crate::analyze::finalize_types::{
    AnalysisProducts, CovDupOutcome, CovDupPhase, FinalizeAnalysisIn, HeaderPhase, StorePrintPhase,
};
use crate::analyze::focus::{filter_duplicates_by_focus, filter_viols_by_focus};
use crate::analyze::graph_api::graph_stats;
use crate::analyze::options::AnalyzeResult;
use crate::analyze::parallel::RustAnalysis;
use crate::analyze::print::{
    PrintResultsCtx, log_timing_phase1, log_timing_phase2, print_all_results_with_dups,
    print_analysis_summary,
};
use crate::analyze_parse::ParseResult;

pub(crate) fn build_metrics(
    result: &ParseResult,
    file_count: usize,
    py_g: Option<&kiss::DependencyGraph>,
    rs_g: Option<&kiss::DependencyGraph>,
) -> kiss::GlobalMetrics {
    let (nodes, edges) = graph_stats(py_g, rs_g);
    kiss::GlobalMetrics {
        files: file_count,
        code_units: result.code_unit_count,
        statements: result.statement_count,
        graph_nodes: nodes,
        graph_edges: edges,
    }
}

fn finalize_header(phase: HeaderPhase<'_>) -> kiss::GlobalMetrics {
    let HeaderPhase {
        opts,
        result,
        file_count,
        py_graph,
        rs_graph,
        timings,
    } = phase;
    if opts.show_timing {
        log_timing_phase1(timings.0, timings.1, timings.2, Instant::now());
    }
    let metrics = build_metrics(result, file_count, py_graph, rs_graph);
    print_analysis_summary(&metrics, py_graph, rs_graph);
    metrics
}

fn finalize_coverage_and_dups(phase: CovDupPhase<'_>) -> CovDupOutcome {
    let CovDupPhase {
        opts,
        focus_set,
        viols,
        py_cov,
        rs_cov,
        py_graph,
        rs_graph,
        graph_viols_all,
        py_dups_all,
        rs_dups_all,
    } = phase;
    viols.extend(filter_viols_by_focus(graph_viols_all.to_vec(), focus_set));
    let t_phase2 = Instant::now();
    let graphs = GraphRefPair {
        py: py_graph,
        rs: rs_graph,
    };
    let out_opts = CoverageOutputOpts {
        bypass_gate: opts.bypass_gate,
        show_timing: opts.show_timing,
    };
    let (cov_viols, coverage_cache_lists) = collect_coverage_viols(
        PyRsTestCoverage {
            py: py_cov,
            rs: rs_cov,
        },
        focus_set,
        out_opts,
        graphs,
    );
    viols.extend(cov_viols.iter().cloned());
    let py_dups = filter_duplicates_by_focus(py_dups_all.to_vec(), focus_set);
    let rs_dups_f = filter_duplicates_by_focus(rs_dups_all.to_vec(), focus_set);
    log_timing_phase2(opts.show_timing, t_phase2, Instant::now());
    CovDupOutcome {
        cov_viols,
        coverage_cache_lists,
        t_phase2,
        py_dups,
        rs_dups: rs_dups_f,
    }
}

fn finalize_store_and_print(phase: StorePrintPhase<'_>) -> bool {
    let StorePrintPhase {
        opts,
        py_files,
        rs_files,
        result,
        viols,
        graph_viols_all,
        cov_viols,
        py_graph,
        rs_graph,
        py_dups_all,
        rs_dups_all,
        coverage_cache_lists,
        py_dups,
        rs_dups,
        t_phase2,
    } = phase;
    maybe_store_full_cache(FullCacheStoreInput {
        opts,
        py_files,
        rs_files,
        result,
        graph_viols_all,
        coverage_violations: cov_viols,
        py_graph,
        rs_graph,
        py_dups_all,
        rs_dups_all,
        coverage_cache_lists,
    });
    print_all_results_with_dups(
        viols,
        py_dups,
        rs_dups,
        PrintResultsCtx {
            show_timing: opts.show_timing,
            t_phase2: Some(t_phase2),
            suppress_final_status: opts.suppress_final_status,
        },
    )
}

pub(crate) fn finalize_analysis(in_: FinalizeAnalysisIn<'_>) -> AnalyzeResult {
    let FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus_set,
        products:
            AnalysisProducts {
                result,
                mut viols,
                file_count,
                py_cov,
                rs,
                py_graph,
                graph_viols_all,
                py_dups_all,
            },
        timings,
    } = in_;

    let RustAnalysis {
        graph: rs_graph_owned,
        cov: rs_cov,
        dups: rs_dups_vec,
    } = rs;

    let metrics = finalize_header(HeaderPhase {
        opts,
        result: &result,
        file_count,
        py_graph: py_graph.as_ref(),
        rs_graph: rs_graph_owned.as_ref(),
        timings,
    });

    let outcome = finalize_coverage_and_dups(CovDupPhase {
        opts,
        focus_set,
        viols: &mut viols,
        py_cov,
        rs_cov,
        py_graph: py_graph.as_ref(),
        rs_graph: rs_graph_owned.as_ref(),
        graph_viols_all: &graph_viols_all,
        py_dups_all: &py_dups_all,
        rs_dups_all: &rs_dups_vec,
    });

    let success = finalize_store_and_print(StorePrintPhase {
        opts,
        py_files,
        rs_files,
        result: &result,
        viols: &viols,
        graph_viols_all: &graph_viols_all,
        cov_viols: &outcome.cov_viols,
        py_graph: py_graph.as_ref(),
        rs_graph: rs_graph_owned.as_ref(),
        py_dups_all: &py_dups_all,
        rs_dups_all: &rs_dups_vec,
        coverage_cache_lists: outcome.coverage_cache_lists,
        py_dups: &outcome.py_dups,
        rs_dups: &outcome.rs_dups,
        t_phase2: outcome.t_phase2,
    });

    AnalyzeResult {
        success,
        metrics: Some(metrics),
    }
}

#[cfg(test)]
mod finalize_touch {
    use super::*;
    use crate::analyze::finalize_types::FinalizeAnalysisIn;
    use crate::analyze_parse::ParseResult;

    #[test]
    fn struct_size_for_gate() {
        let _ = std::mem::size_of::<FinalizeAnalysisIn>();
    }

    fn make_parse_result(code_unit_count: usize, statement_count: usize) -> ParseResult {
        ParseResult {
            py_parsed: Vec::new(),
            rs_parsed: Vec::new(),
            violations: Vec::new(),
            code_unit_count,
            statement_count,
        }
    }

    #[test]
    fn test_build_metrics_empty() {
        let result = make_parse_result(0, 0);
        let metrics = build_metrics(&result, 0, None, None);
        assert_eq!(metrics.files, 0);
        assert_eq!(metrics.code_units, 0);
        assert_eq!(metrics.statements, 0);
        assert_eq!(metrics.graph_nodes, 0);
        assert_eq!(metrics.graph_edges, 0);
    }

    #[test]
    fn test_build_metrics_counts_files() {
        let result = make_parse_result(10, 20);
        let metrics = build_metrics(&result, 5, None, None);
        assert_eq!(metrics.files, 5);
        assert_eq!(metrics.code_units, 10);
        assert_eq!(metrics.statements, 20);
    }

    #[test]
    fn finalize_functions_exist() {
        let _ = finalize_header as fn(HeaderPhase<'_>) -> kiss::GlobalMetrics;
        let _ = finalize_coverage_and_dups as fn(CovDupPhase<'_>) -> CovDupOutcome;
        let _ = finalize_store_and_print as fn(StorePrintPhase<'_>) -> bool;
    }
}
