use crate::analyze::finalize::{AnalysisProducts, FinalizeAnalysisIn, finalize_analysis};
use crate::analyze::focus::{FocusFilter, filter_viols_by_focus};
use crate::analyze::options::{AnalyzeOptions, AnalyzeResult};
use crate::analyze::parallel::{ParallelPyIn, run_parallel_py_analysis, run_rust_analysis};
use crate::analyze::params::RunAnalyzeUncached;
use crate::analyze::print::log_parse_timing;
use crate::analyze_parse::{ParseAllTimedParams, ParseResult, parse_all_timed};
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

fn build_python_metric_stats(
    py_parsed: &[ParsedFile],
    py_graph: Option<&DependencyGraph>,
    roles: &kiss::code_roles::SourceRoleIndex,
) -> kiss::MetricStats {
    let refs: Vec<_> = py_parsed
        .iter()
        .filter(|p| !kiss::code_roles::is_test_only_file(roles, &p.path))
        .collect();
    let mut stats = if refs.is_empty() {
        kiss::MetricStats::default()
    } else {
        kiss::MetricStats::collect(&refs)
    };
    if let Some(graph) = py_graph {
        stats.collect_graph_metrics(graph);
    }
    stats
}

fn build_rust_metric_stats(
    rs_parsed: &[ParsedRustFile],
    rs_graph: Option<&DependencyGraph>,
    roles: &kiss::code_roles::SourceRoleIndex,
) -> kiss::MetricStats {
    let refs: Vec<_> = rs_parsed
        .iter()
        .filter(|p| !kiss::code_roles::is_test_only_file(roles, &p.path))
        .collect();
    let mut stats = if refs.is_empty() {
        kiss::MetricStats::default()
    } else {
        kiss::MetricStats::collect_rust_with_roles(&refs, Some(roles))
    };
    if let Some(graph) = rs_graph {
        stats.collect_graph_metrics(graph);
    }
    stats
}

pub(crate) fn run_full_pipeline(
    in_: FullPipelineInput<'_>,
) -> Result<FullPipelineResult, kiss::code_roles::RoleBuildError> {
    let (result, parse_timing) = parse_all_timed(ParseAllTimedParams {
        py_files: in_.py_files,
        rs_files: in_.rs_files,
        py_config: in_.opts.py_config,
        rs_config: in_.opts.rs_config,
        show_timing: in_.opts.show_timing,
    })?;
    Ok(run_full_pipeline_with_parse(FullPipelineWithParseInput {
        opts: in_.opts,
        focus: in_.focus,
        result,
        parse_timing,
        timings: (in_.t0, in_.t1, in_.t2),
    }))
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
        result
            .violations
            .extend(kiss::collect_comment_violations_with_roles(
                &result.py_parsed,
                &result.rs_parsed,
                Some(&result.roles),
            ));
    }
    result
        .violations
        .extend(kiss::collect_doc_violations_with_roles(
            &result.py_parsed,
            &result.rs_parsed,
            &opts.gate_config.docs_allowed,
            &crate::analyze_cache::repo_root_for_universe(opts.universe),
            Some(&result.roles),
        ));
    let file_count = result
        .py_parsed
        .iter()
        .filter(|p| !kiss::code_roles::is_test_only_file(&result.roles, &p.path))
        .count()
        + result
            .rs_parsed
            .iter()
            .filter(|p| !kiss::code_roles::is_test_only_file(&result.roles, &p.path))
            .count();
    let viols = filter_viols_by_focus(result.violations.clone(), focus);
    let rs = run_rust_analysis(&result.rs_parsed, opts.gate_config, &result.roles);
    let ((py_graph, graph_viols_all), py_dups_all) = run_parallel_py_analysis(ParallelPyIn {
        py_parsed: &result.py_parsed,
        rs_graph: rs.graph.as_ref(),
        opts,
        file_count,
        roles: &result.roles,
    });
    let rs_dups_all = rs.dups.clone();

    let py_stats = build_python_metric_stats(&result.py_parsed, py_graph.as_ref(), &result.roles);
    let rs_stats = build_rust_metric_stats(&result.rs_parsed, rs.graph.as_ref(), &result.roles);

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
    let (result, parse_timing) = match parse_all_timed(ParseAllTimedParams {
        py_files,
        rs_files,
        py_config: opts.py_config,
        rs_config: opts.rs_config,
        show_timing: opts.show_timing,
    }) {
        Ok(ok) => ok,
        Err(err) => {
            eprintln!("{err}");
            return AnalyzeResult { success: false };
        }
    };

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
            roles: kiss::code_roles::SourceRoleIndex::empty(),
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
