use crate::analyze::coverage::{
    CoverageOutputOpts, GraphRefPair, PyRsTestCoverage, collect_coverage_viols,
};
use crate::analyze::finalize::{AnalysisProducts, FinalizeAnalysisIn, finalize_analysis};
use crate::analyze::focus::filter_viols_by_focus;
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult};
use crate::analyze::parallel::{ParallelPyIn, run_parallel_py_analysis, run_rust_analysis};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::print::log_parse_timing;
use crate::analyze_parse::{ParseAllTimedParams, ParseResult, parse_all_timed};
use kiss::check_universe_cache::CachedCoverageItem;
use kiss::cli_output::file_coverage_map;
use kiss::{DependencyGraph, DuplicateCluster, MetricStats, ParsedFile, ParsedRustFile};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

pub(crate) struct FullPipelineResult {
    pub result: ParseResult,
    pub viols: Vec<kiss::Violation>,
    pub file_count: usize,
    pub py_graph: Option<DependencyGraph>,
    pub rs: crate::analyze::parallel::RustAnalysis,
    pub graph_viols_all: Vec<kiss::Violation>,
    pub cov_viols: Vec<kiss::Violation>,
    pub py_cov: kiss::TestRefAnalysis,
    pub py_dups_all: Vec<DuplicateCluster>,
    pub rs_dups_all: Vec<DuplicateCluster>,
    pub py_stats: MetricStats,
    pub rs_stats: MetricStats,
    pub coverage_cache_lists: Option<(Vec<CachedCoverageItem>, Vec<CachedCoverageItem>)>,
    pub timings: (Instant, Instant, Instant),
}

pub(crate) struct FullPipelineInput<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus_set: &'a HashSet<PathBuf>,
    pub t0: Instant,
    pub t1: Instant,
    pub t2: Instant,
}

fn build_metric_stats<T, FCollect, FPath>(
    parsed: &[T],
    graph: Option<&DependencyGraph>,
    defs: Vec<(PathBuf, String, usize)>,
    unreferenced: Vec<(PathBuf, String, usize)>,
    path_of: FPath,
    init: FCollect,
) -> MetricStats
where
    FCollect: Fn(&[T]) -> MetricStats,
    FPath: Fn(&T) -> &PathBuf,
{
    let mut stats = init(parsed);
    if let Some(graph) = graph {
        stats.collect_graph_metrics(graph);
    }
    let by_file = file_coverage_map(&defs, &unreferenced);
    let file_coverages = parsed
        .iter()
        .map(|item| by_file.get(path_of(item)).copied().unwrap_or(100))
        .collect::<Vec<_>>();
    stats.extend_inv_test_coverage(file_coverages);
    stats
}

fn build_python_metric_stats(
    parsed: &[ParsedFile],
    graph: Option<&DependencyGraph>,
    coverage: &kiss::TestRefAnalysis,
) -> MetricStats {
    let defs = coverage
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect::<Vec<_>>();
    let unreferenced = coverage
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect::<Vec<_>>();
    build_metric_stats(
        parsed,
        graph,
        defs,
        unreferenced,
        |item| &item.path,
        |files| {
            let refs: Vec<_> = files.iter().collect();
            MetricStats::collect(&refs)
        },
    )
}

fn build_rust_metric_stats(
    parsed: &[ParsedRustFile],
    graph: Option<&DependencyGraph>,
    coverage: &kiss::RustTestRefAnalysis,
) -> MetricStats {
    let defs = coverage
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect::<Vec<_>>();
    let unreferenced = coverage
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect::<Vec<_>>();
    build_metric_stats(
        parsed,
        graph,
        defs,
        unreferenced,
        |item| &item.path,
        |files| {
            let refs: Vec<_> = files.iter().collect();
            MetricStats::collect_rust(&refs)
        },
    )
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
        focus_set: in_.focus_set,
        result,
        parse_timing,
        timings: (in_.t0, in_.t1, in_.t2),
    })
}

struct FullPipelineWithParseInput<'a> {
    opts: &'a AnalyzeOptions<'a>,
    focus_set: &'a HashSet<PathBuf>,
    result: ParseResult,
    parse_timing: String,
    timings: (Instant, Instant, Instant),
}

fn run_full_pipeline_with_parse(in_: FullPipelineWithParseInput<'_>) -> FullPipelineResult {
    let opts = in_.opts;
    let focus_set = in_.focus_set;
    let timings = in_.timings;
    log_parse_timing(opts.show_timing, &in_.parse_timing);
    let result = in_.result;
    let file_count = result.py_parsed.len() + result.rs_parsed.len();
    let viols = filter_viols_by_focus(result.violations.clone(), focus_set);
    let rs = run_rust_analysis(&result.rs_parsed, opts.gate_config, None);
    let ((py_graph, graph_viols_all), (py_cov, py_dups_all)) =
        run_parallel_py_analysis(ParallelPyIn {
            py_parsed: &result.py_parsed,
            rs_graph: rs.graph.as_ref(),
            opts,
            file_count,
            cached_py_cov: None,
        });
    let rs_dups_all = rs.dups.clone();

    let py_stats = build_python_metric_stats(&result.py_parsed, py_graph.as_ref(), &py_cov);
    let rs_stats = build_rust_metric_stats(&result.rs_parsed, rs.graph.as_ref(), &rs.cov);

    let (cov_viols, coverage_cache_lists) = if opts.show_timing {
        (Vec::new(), None)
    } else {
        let (cov_viols, cache_lists) = collect_coverage_viols(
            PyRsTestCoverage {
                py: py_cov.clone(),
                rs: rs.cov.clone(),
            },
            focus_set,
            CoverageOutputOpts {
                bypass_gate: opts.bypass_gate,
                show_timing: false,
            },
            GraphRefPair {
                py: py_graph.as_ref(),
                rs: rs.graph.as_ref(),
            },
        );
        (cov_viols, cache_lists)
    };

    FullPipelineResult {
        result,
        viols,
        file_count,
        py_graph,
        rs,
        graph_viols_all,
        cov_viols,
        py_cov,
        py_dups_all,
        rs_dups_all,
        py_stats,
        rs_stats,
        coverage_cache_lists,
        timings,
    }
}

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

    if !opts.bypass_gate && opts.gate_config.test_coverage_threshold > 0 {
        let py_refs = result.py_parsed.iter().collect::<Vec<_>>();
        let rs_refs = result.rs_parsed.iter().collect::<Vec<_>>();
        let py_cov = kiss::analyze_test_refs_quick(&py_refs);
        let rs_cov = kiss::analyze_rust_test_refs(&rs_refs, None);
        if let Some(early) = crate::analyze::coverage_gate::evaluate_gate(
            &py_cov,
            &rs_cov,
            focus_set,
            opts.gate_config.test_coverage_threshold,
        ) {
            log_parse_timing(opts.show_timing, &parse_timing);
            return early;
        }
    }

    let pipeline = run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts,
        focus_set,
        result,
        parse_timing,
        timings: (t0, t1, Instant::now()),
    });

    finalize_analysis(FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus_set,
        products: AnalysisProducts {
            result: pipeline.result,
            viols: pipeline.viols,
            file_count: pipeline.file_count,
            py_cov: pipeline.py_cov,
            cov_viols: pipeline.cov_viols,
            coverage_cache_lists: pipeline.coverage_cache_lists,
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
