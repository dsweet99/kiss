use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use kiss::Violation;
use kiss::check_universe_cache::CachedCoverageItem;

use crate::analyze::options::AnalyzeOptions;
use crate::analyze::parallel::RustAnalysis;
use crate::analyze_parse::ParseResult;

pub(crate) type CoverageCachePair = (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>);

pub(crate) struct AnalysisProducts {
    pub result: ParseResult,
    pub viols: Vec<Violation>,
    pub file_count: usize,
    pub py_cov: kiss::TestRefAnalysis,
    pub cov_viols: Vec<Violation>,
    pub coverage_cache_lists: Option<CoverageCachePair>,
    pub py_stats: Option<kiss::MetricStats>,
    pub rs_stats: Option<kiss::MetricStats>,
    pub rs: RustAnalysis,
    pub py_graph: Option<kiss::DependencyGraph>,
    pub graph_viols_all: Vec<Violation>,
    pub py_dups_all: Vec<kiss::DuplicateCluster>,
}

pub(crate) struct FinalizeAnalysisIn<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus_set: &'a HashSet<PathBuf>,
    pub products: AnalysisProducts,
    pub timings: (Instant, Instant, Instant),
}

pub(crate) struct HeaderPhase<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub result: &'a ParseResult,
    pub file_count: usize,
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub timings: (Instant, Instant, Instant),
}

pub(crate) struct CovDupPhase<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub focus_set: &'a HashSet<PathBuf>,
    pub viols: &'a mut Vec<Violation>,
    pub py_cov: kiss::TestRefAnalysis,
    pub rs_cov: kiss::RustTestRefAnalysis,
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub precomputed_cov_viols: Vec<Violation>,
    pub precomputed_coverage_cache_lists: Option<CoverageCachePair>,
    pub graph_viols_all: &'a [Violation],
    pub py_dups_all: &'a [kiss::DuplicateCluster],
    pub rs_dups_all: &'a [kiss::DuplicateCluster],
}

pub(crate) struct CovDupOutcome {
    pub cov_viols: Vec<Violation>,
    pub coverage_cache_lists: Option<CoverageCachePair>,
    pub t_phase2: Instant,
    pub py_dups: Vec<kiss::DuplicateCluster>,
    pub rs_dups: Vec<kiss::DuplicateCluster>,
}

pub(crate) struct StorePrintPhase<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus_set: &'a HashSet<PathBuf>,
    pub result: &'a ParseResult,
    pub viols: &'a [Violation],
    pub graph_viols_all: &'a [Violation],
    pub cov_viols: &'a [Violation],
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub py_dups_all: &'a [kiss::DuplicateCluster],
    pub rs_dups_all: &'a [kiss::DuplicateCluster],
    pub coverage_cache_lists: Option<CoverageCachePair>,
    pub py_stats: Option<&'a kiss::MetricStats>,
    pub rs_stats: Option<&'a kiss::MetricStats>,
    pub py_dups: &'a [kiss::DuplicateCluster],
    pub rs_dups: &'a [kiss::DuplicateCluster],
    pub t_phase2: Instant,
}
