use crate::analyze::finalize::{finalize_analysis, AnalysisProducts, FinalizeAnalysisIn};
use crate::analyze::focus::{filter_viols_by_focus, FocusFilter};
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult};
use crate::analyze::parallel::{run_parallel_py_analysis, run_rust_analysis, ParallelPyIn};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::print::log_parse_timing;
use crate::analyze_parse::{parse_all_timed, ParseAllTimedParams, ParseResult};
use kiss::{DependencyGraph, ParsedFile, ParsedRustFile};
use std::path::PathBuf;
use std::time::Instant;

pub(crate) struct FullPipelineResult {
    pub result: ParseResult,
    pub viols: Vec<kiss::Violation>,
    pub file_count: usize,
    pub py_graph: Option<DependencyGraph>,
    pub rs: crate::analyze::parallel::RustAnalysis,
    pub graph_viols_all: Vec<kiss::Violation>,
    pub py_dups_all: Vec<kiss::DuplicateCluster>,
    pub rs_dups_all: Vec<kiss::DuplicateCluster>,
    pub py_stats: kiss::MetricStats,
    pub rs_stats: kiss::MetricStats,
    pub timings: (Instant, Instant, Instant),
}

pub(crate) struct FullPipelineInput<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus: &'a FocusFilter,
    pub t0: Instant,
    pub t1: Instant,
    pub t2: Instant,
}

fn build_metric_stats<T, FCollect>(
    parsed: &[T],
    graph: Option<&DependencyGraph>,
    init: FCollect,
) -> kiss::MetricStats
where
    FCollect: FnOnce(&[T]) -> kiss::MetricStats,
{
    let mut stats = if parsed.is_empty() {
        kiss::MetricStats::default()
    } else {
        init(parsed)
    };
    if let Some(graph) = graph {
        stats.collect_graph_metrics(graph);
    }
    stats
}

fn build_python_metric_stats(
    py_parsed: &[ParsedFile],
    py_graph: Option<&DependencyGraph>,
) -> kiss::MetricStats {
    build_metric_stats(py_parsed, py_graph, |files| {
        let refs: Vec<_> = files.iter().collect();
        kiss::MetricStats::collect(&refs)
    })
}

fn build_rust_metric_stats(
    rs_parsed: &[ParsedRustFile],
    rs_graph: Option<&DependencyGraph>,
) -> kiss::MetricStats {
    build_metric_stats(rs_parsed, rs_graph, |files| {
        let refs: Vec<_> = files.iter().collect();
        kiss::MetricStats::collect_rust(&refs)
    })
}

pub(crate) fn run_full_pipeline(in_: FullPipelineInput<'_>) -> FullPipelineResult {
    let (result, parse_timing) = parse_all_timed(ParseAllTimedParams {
        py_files: in_.py_files,
        rs_files: in_.rs_files,
        py_config: in_.opts.py_config,
        rs_config: in_.opts.rs_config,
        show_timing: in_.opts.show_timing,
    });
    run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts: in_.opts,
        focus: in_.focus,
        result,
        parse_timing,
        timings: (in_.t0, in_.t1, in_.t2),
    })
}

struct FullPipelineWithParseInput<'a> {
    opts: &'a AnalyzeOptions<'a>,
    focus: &'a FocusFilter,
    result: ParseResult,
    parse_timing: String,
    timings: (Instant, Instant, Instant),
}

fn run_full_pipeline_with_parse(in_: FullPipelineWithParseInput<'_>) -> FullPipelineResult {
    let opts = in_.opts;
    let focus = in_.focus;
    let timings = in_.timings;
    log_parse_timing(opts.show_timing, &in_.parse_timing);
    let mut result = in_.result;
    if opts.gate_config.comment_removal_enabled {
        result.violations.extend(kiss::collect_comment_violations(
            &result.py_parsed,
            &result.rs_parsed,
        ));
    }
    result.violations.extend(kiss::collect_doc_violations(
        &result.py_parsed,
        &result.rs_parsed,
        &opts.gate_config.docs_allowed,
        &crate::analyze_cache::repo_root_for_universe(opts.universe),
    ));
    let file_count = result.py_parsed.len() + result.rs_parsed.len();
    let viols = filter_viols_by_focus(result.violations.clone(), focus);
    let rs = run_rust_analysis(&result.rs_parsed, opts.gate_config);
    let ((py_graph, graph_viols_all), py_dups_all) = run_parallel_py_analysis(ParallelPyIn {
        py_parsed: &result.py_parsed,
        rs_graph: rs.graph.as_ref(),
        opts,
        file_count,
    });
    let rs_dups_all = rs.dups.clone();

    let py_stats = build_python_metric_stats(&result.py_parsed, py_graph.as_ref());
    let rs_stats = build_rust_metric_stats(&result.rs_parsed, rs.graph.as_ref());

    FullPipelineResult {
        result,
        viols,
        file_count,
        py_graph,
        rs,
        graph_viols_all,
        py_dups_all,
        rs_dups_all,
        py_stats,
        rs_stats,
        timings,
    }
}

pub(crate) fn run_analyze_uncached(in_: RunAnalyzeUncached<'_>) -> AnalyzeResult {
    let RunAnalyzeUncached {
        opts,
        py_files,
        rs_files,
        focus,
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

    let pipeline = run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts,
        focus,
        result,
        parse_timing,
        timings: (t0, t1, Instant::now()),
    });

    finalize_analysis(FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus,
        products: AnalysisProducts {
            result: pipeline.result,
            viols: pipeline.viols,
            file_count: pipeline.file_count,
            py_stats: Some(pipeline.py_stats),
            rs_stats: Some(pipeline.rs_stats),
            rs: pipeline.rs,
            py_graph: pipeline.py_graph,
            graph_viols_all: pipeline.graph_viols_all,
            py_dups_all: pipeline.py_dups_all,
        },
        timings: pipeline.timings,
    })
}

#[cfg(test)]
pub(crate) fn empty_full_pipeline_result_for_tests() -> FullPipelineResult {
    use crate::analyze::parallel::RustAnalysis;
    let now = Instant::now();
    FullPipelineResult {
        result: ParseResult {
            py_parsed: Vec::new(),
            rs_parsed: Vec::new(),
            violations: Vec::new(),
            code_unit_count: 0,
            statement_count: 0,
        },
        viols: Vec::new(),
        file_count: 0,
        py_graph: None,
        rs: RustAnalysis {
            graph: None,
            dups: Vec::new(),
        },
        graph_viols_all: Vec::new(),
        py_dups_all: Vec::new(),
        rs_dups_all: Vec::new(),
        py_stats: kiss::MetricStats::default(),
        rs_stats: kiss::MetricStats::default(),
        timings: (now, now, now),
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;

    #[test]
    fn full_pipeline_input_is_coverage_free() {
        let _ = std::mem::size_of::<FullPipelineInput<'_>>();
        let _ = std::mem::size_of::<FullPipelineResult>();
        let _ = empty_full_pipeline_result_for_tests();
    }
}
