//! Bundles for pipeline entry points (kiss argument thresholds).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use kiss::Violation;

use crate::analyze::options::AnalyzeOptions;

/// Inputs for [`crate::analyze::pipeline::run_analyze_uncached`].
pub(crate) struct RunAnalyzeUncached<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus_set: &'a HashSet<PathBuf>,
    pub t0: Instant,
    pub t1: Instant,
}

/// Inputs for [`crate::analyze::gated::run_gated_analysis`].
pub(crate) struct GatedAnalysis<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub focus_set: &'a HashSet<PathBuf>,
    pub parsed: (crate::analyze_parse::ParseResult, Vec<Violation>, usize),
    pub timings: (Instant, Instant, Instant),
}

#[cfg(test)]
mod params_coverage_touch {
    use super::*;
    use std::collections::HashSet;
    use std::time::Instant;

    use crate::analyze_parse::ParseResult;
    use kiss::GateConfig;

    #[test]
    fn gated_analysis_param_struct_referenced() {
        let py_cfg = kiss::Config::python_defaults();
        let rs_cfg = kiss::Config::rust_defaults();
        let gate = GateConfig::default();
        let focus: Vec<String> = vec![];
        let opts = crate::analyze::options::AnalyzeOptions {
            universe: "/tmp",
            focus_paths: &focus,
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: false,
        };
        let parsed = ParseResult {
            py_parsed: vec![],
            rs_parsed: vec![],
            violations: vec![],
            code_unit_count: 0,
            statement_count: 0,
        };
        let _bundle = GatedAnalysis {
            opts: &opts,
            py_files: &[],
            rs_files: &[],
            focus_set: &HashSet::new(),
            parsed: (parsed, vec![], 0),
            timings: (Instant::now(), Instant::now(), Instant::now()),
        };
    }
}
