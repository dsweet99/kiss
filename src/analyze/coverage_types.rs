use std::collections::HashSet;
use std::path::PathBuf;

use kiss::{GateConfig, ParsedFile, ParsedRustFile};

/// Owned Python + Rust test-reference analyses for coverage merging.
pub(crate) struct PyRsTestCoverage {
    pub py: kiss::TestRefAnalysis,
    pub rs: kiss::RustTestRefAnalysis,
}

/// Definition identity and per-file coverage percent for building a violation.
pub(crate) struct CoverageViolationSpec {
    pub file: PathBuf,
    pub name: String,
    pub line: usize,
    pub file_pct: usize,
}

/// Inputs for [`crate::analyze::coverage_gate::check_coverage_gate`].
pub struct CheckCoverageGateParams<'a> {
    pub py_parsed: &'a [ParsedFile],
    pub rs_parsed: &'a [ParsedRustFile],
    pub gate_config: &'a GateConfig,
    pub focus_set: &'a HashSet<PathBuf>,
    pub show_timing: bool,
}
