//! Bundles for pipeline entry points (kiss argument thresholds).

use std::path::PathBuf;
use std::time::Instant;

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

#[cfg(test)]
mod params_tests {
    use super::*;

    #[test]
    fn run_analyze_uncached_fields_are_named() {

        let _ = std::mem::size_of::<RunAnalyzeUncached<'_>>();
    }
}
