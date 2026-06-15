use std::path::PathBuf;
use std::time::Instant;

use kiss::Violation;
use kiss::check_universe_cache::CachedCoverageItem;

use crate::analyze::focus::FocusFilter;

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
    pub focus: &'a FocusFilter,
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
    pub focus: &'a FocusFilter,
    pub viols: &'a mut Vec<Violation>,
    pub py_cov: kiss::TestRefAnalysis,
    pub rs_cov: kiss::RustTestRefAnalysis,
    pub py_parsed: &'a [kiss::ParsedFile],
    pub rs_parsed: &'a [kiss::ParsedRustFile],
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
    pub focus: &'a FocusFilter,
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

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use crate::analyze::parallel::RustAnalysis;
    use kiss::{RustTestRefAnalysis, TestRefAnalysis};
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    impl AnalysisProducts {
        fn witness() -> Self {
            Self {
                result: ParseResult {
                    py_parsed: vec![],
                    rs_parsed: vec![],
                    violations: vec![],
                    code_unit_count: 0,
                    statement_count: 0,
                },
                viols: vec![],
                file_count: 0,
                py_cov: TestRefAnalysis {
                    definitions: vec![],
                    test_references: HashSet::new(),
                    call_references: HashSet::new(),
                    unreferenced: vec![],
                    coverage_map: HashMap::new(),
                },
                cov_viols: vec![],
                coverage_cache_lists: None,
                py_stats: None,
                rs_stats: None,
                rs: RustAnalysis {
                    graph: None,
                    cov: RustTestRefAnalysis {
                        definitions: vec![],
                        test_references: HashSet::new(),
                        call_references: HashSet::new(),
                        propagated_references: HashSet::new(),
                        unreferenced: vec![],
                        coverage_map: HashMap::new(),
                    },
                    dups: vec![],
                },
                py_graph: None,
                graph_viols_all: vec![],
                py_dups_all: vec![],
            }
        }
    }

    impl<'a> FinalizeAnalysisIn<'a> {
        fn witness() {}
    }

    impl<'a> HeaderPhase<'a> {
        fn witness() {}
    }

    impl<'a> CovDupPhase<'a> {
        fn witness() {}
    }

    impl CovDupOutcome {
        fn witness() -> Self {
            Self {
                cov_viols: vec![],
                coverage_cache_lists: None,
                t_phase2: Instant::now(),
                py_dups: vec![],
                rs_dups: vec![],
            }
        }
    }

    impl<'a> StorePrintPhase<'a> {
        fn witness() {}
    }

    #[test]
    fn witness_finalize_types() {
        let _ = AnalysisProducts::witness();
        FinalizeAnalysisIn::witness();
        HeaderPhase::witness();
        CovDupPhase::witness();
        let _ = CovDupOutcome::witness();
        StorePrintPhase::witness();
    }
}
