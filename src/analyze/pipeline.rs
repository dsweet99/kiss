use crate::analyze::finalize::{AnalysisProducts, FinalizeAnalysisIn, finalize_analysis};
use crate::analyze::focus::filter_viols_by_focus;
use crate::analyze::gated::run_gated_analysis;
use crate::analyze::options::AnalyzeResult;
use crate::analyze::parallel::{ParallelPyIn, run_parallel_py_analysis, run_rust_analysis};
use crate::analyze::params::{GatedAnalysis, RunAnalyzeUncached};
use crate::analyze::print::log_parse_timing;
use crate::analyze_parse::{ParseAllTimedParams, parse_all_timed};

pub(crate) fn run_analyze_uncached(in_: RunAnalyzeUncached<'_>) -> AnalyzeResult {
    let RunAnalyzeUncached {
        opts,
        py_files,
        rs_files,
        focus_set,
        t0,
        t1,
    } = in_;
    let (result, parse_timing) = parse_all_timed(ParseAllTimedParams {
        py_files,
        rs_files,
        py_config: opts.py_config,
        rs_config: opts.rs_config,
        show_timing: opts.show_timing,
    });
    let t2 = std::time::Instant::now();
    log_parse_timing(opts.show_timing, &parse_timing);
    let viols = filter_viols_by_focus(result.violations.clone(), focus_set);
    let file_count = result.py_parsed.len() + result.rs_parsed.len();

    if !opts.bypass_gate {
        return run_gated_analysis(GatedAnalysis {
            opts,
            py_files,
            rs_files,
            focus_set,
            parsed: (result, viols, file_count),
            timings: (t0, t1, t2),
        });
    }

    let rs = run_rust_analysis(&result.rs_parsed, opts.gate_config, None);
    let ((py_graph, graph_viols_all), (py_cov, py_dups_all)) =
        run_parallel_py_analysis(ParallelPyIn {
            py_parsed: &result.py_parsed,
            rs_graph: rs.graph.as_ref(),
            opts,
            file_count,
            cached_py_cov: None,
        });

    finalize_analysis(FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus_set,
        products: AnalysisProducts {
            result,
            viols,
            file_count,
            py_cov,
            rs,
            py_graph,
            graph_viols_all,
            py_dups_all,
        },
        timings: (t0, t1, t2),
    })
}
