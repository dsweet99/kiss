use super::*;
use crate::analyze::cache::{FullCacheStoreInput, maybe_store_full_cache};
use crate::analyze::coverage::{
    CoverageOutputOpts, GraphRefPair, PyRsTestCoverage, collect_coverage_viols,
};
use crate::analyze::print::log_parse_timing;
use crate::analyze_parse::ParseResult;
use kiss::DependencyGraph;
use std::path::PathBuf;

pub(super) struct CoverageGateFailureCacheIn<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus: &'a FocusFilter,
    pub result: &'a ParseResult,
    pub py_cov: kiss::TestRefAnalysis,
    pub rs_cov: kiss::RustTestRefAnalysis,
    pub rs_graph: Option<&'a DependencyGraph>,
}

pub(super) fn should_store_coverage_gate_failure_cache(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
) -> bool {
    const MAX_UNREFERENCED_UNITS_TO_CACHE: usize = 10_000;
    py_cov.unreferenced.len() + rs_cov.unreferenced.len() <= MAX_UNREFERENCED_UNITS_TO_CACHE
}

pub(super) fn store_coverage_gate_failure_cache(in_: CoverageGateFailureCacheIn<'_>) {
    let py_graph = crate::analyze::graph_api::build_py_graph(&in_.result.py_parsed);
    let (_cov_viols, coverage_cache_lists) = collect_coverage_viols(
        PyRsTestCoverage {
            py: in_.py_cov,
            rs: in_.rs_cov,
        },
        &in_.result.py_parsed,
        &in_.result.rs_parsed,
        in_.focus,
        CoverageOutputOpts {
            bypass_gate: false,
            show_timing: in_.opts.show_timing,
        },
        GraphRefPair {
            py: py_graph.as_ref(),
            rs: in_.rs_graph,
        },
    );
    maybe_store_full_cache(FullCacheStoreInput {
        opts: in_.opts,
        py_files: in_.py_files,
        rs_files: in_.rs_files,
        focus: in_.focus,
        result: in_.result,
        graph_viols_all: &[],
        coverage_violations: &[],
        py_graph: py_graph.as_ref(),
        rs_graph: in_.rs_graph,
        py_dups_all: &[],
        rs_dups_all: &[],
        py_stats: None,
        rs_stats: None,
        coverage_cache_lists,
    });
}

pub(super) struct FinishGateFailureIn<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus: &'a FocusFilter,
    pub result: &'a ParseResult,
    pub parse_timing: &'a str,
    pub py_cov: kiss::TestRefAnalysis,
    pub rs_cov: kiss::RustTestRefAnalysis,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub early: AnalyzeResult,
}

pub(super) fn finish_coverage_gate_failure(in_: FinishGateFailureIn<'_>) -> AnalyzeResult {
    log_parse_timing(in_.opts.show_timing, in_.parse_timing);
    let gate_violations = crate::analyze::coverage_gate::gate_failure_violations_from_runtime(
        &in_.py_cov,
        &in_.rs_cov,
        in_.focus,
        in_.opts.gate_config.test_coverage_threshold,
    );
    let fp = crate::analyze_cache::fingerprint_for_check(
        in_.py_files,
        in_.rs_files,
        in_.opts.py_config,
        in_.opts.rs_config,
        in_.opts.gate_config,
    );
    crate::analyze_cache::store_gate_failure_replay_cache(
        fp,
        in_.opts,
        in_.py_files,
        in_.rs_files,
        in_.focus,
        &gate_violations,
    );
    if should_store_coverage_gate_failure_cache(&in_.py_cov, &in_.rs_cov) {
        store_coverage_gate_failure_cache(CoverageGateFailureCacheIn {
            opts: in_.opts,
            py_files: in_.py_files,
            rs_files: in_.rs_files,
            focus: in_.focus,
            result: in_.result,
            py_cov: in_.py_cov,
            rs_cov: in_.rs_cov,
            rs_graph: in_.rs_graph,
        });
    }
    in_.early
}
