use std::path::PathBuf;
use std::time::Instant;

use kiss::Violation;

use crate::analyze::focus::FocusFilter;
use crate::analyze::options::AnalyzeOptions;
use crate::analyze::parallel::RustAnalysis;
use crate::analyze_parse::ParseResult;

pub(crate) struct AnalysisProducts {
    pub result: ParseResult,
    pub viols: Vec<Violation>,
    pub file_count: usize,
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

pub(crate) struct DupPhase<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub focus: &'a FocusFilter,
    pub viols: &'a mut Vec<Violation>,
    pub graph_viols_all: &'a [Violation],
    pub py_dups_all: &'a [kiss::DuplicateCluster],
    pub rs_dups_all: &'a [kiss::DuplicateCluster],
}

pub(crate) struct DupOutcome {
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
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub py_dups_all: &'a [kiss::DuplicateCluster],
    pub rs_dups_all: &'a [kiss::DuplicateCluster],
    pub py_stats: Option<&'a kiss::MetricStats>,
    pub rs_stats: Option<&'a kiss::MetricStats>,
    pub py_dups: &'a [kiss::DuplicateCluster],
    pub rs_dups: &'a [kiss::DuplicateCluster],
    pub t_phase2: Instant,
}

#[cfg(test)]
mod finalize_types_touch {
    use super::*;
    use crate::analyze_parse::ParseResult;

    #[test]
    fn analysis_products_witness() {
        let _ = AnalysisProducts {
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
            py_stats: None,
            rs_stats: None,
            rs: RustAnalysis {
                graph: None,
                ctx: kiss::ContextDependencyGraph::empty(),
                dups: vec![],
            },
            py_graph: None,
            graph_viols_all: Vec::new(),
            py_dups_all: Vec::new(),
        };
    }
}
