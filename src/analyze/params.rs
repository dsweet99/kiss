//! Bundles for pipeline entry points (kiss argument thresholds).

use std::path::PathBuf;
use std::time::Instant;

use kiss::Violation;

use crate::analyze::focus::FocusFilter;
use crate::analyze::options::AnalyzeOptions;

/// Inputs for [`crate::analyze::pipeline::run_analyze_uncached`].
pub(crate) struct RunAnalyzeUncached<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus: &'a FocusFilter,
    pub t0: Instant,
    pub t1: Instant,
}

/// Inputs for [`crate::analyze::gated::run_gated_analysis`].
pub(crate) struct GatedAnalysis<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus: &'a FocusFilter,
    pub parsed: (crate::analyze_parse::ParseResult, Vec<Violation>, usize),
    pub timings: (Instant, Instant, Instant),
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl RunAnalyzeUncached<'_> {
        fn witness() {}
    }

    impl GatedAnalysis<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_params_types() {
        RunAnalyzeUncached::witness();
        GatedAnalysis::witness();
    }
}

