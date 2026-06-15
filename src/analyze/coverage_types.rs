use std::path::PathBuf;

use crate::analyze::FocusFilter;
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
    pub focus: &'a FocusFilter,
    pub show_timing: bool,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use kiss::{GateConfig, RustTestRefAnalysis, TestRefAnalysis};
    use std::collections::HashMap;

    impl PyRsTestCoverage {
        fn witness() -> Self {
            Self {
                py: TestRefAnalysis {
                    definitions: Vec::new(),
                    test_references: Default::default(),
                    call_references: Default::default(),
                    unreferenced: Vec::new(),
                    coverage_map: HashMap::new(),
                },
                rs: RustTestRefAnalysis {
                    definitions: Vec::new(),
                    test_references: Default::default(),
                    call_references: Default::default(),
                    propagated_references: Default::default(),
                    unreferenced: Vec::new(),
                    coverage_map: HashMap::new(),
                },
            }
        }
    }

    impl CoverageViolationSpec {
        fn witness() -> Self {
            Self {
                file: std::path::PathBuf::from("witness.py"),
                name: "witness".into(),
                line: 1,
                file_pct: 100,
            }
        }
    }

    impl<'a> CheckCoverageGateParams<'a> {
        fn witness() {}
    }

    #[test]
    fn witness_coverage_types() {
        let gate = GateConfig::default();
        let py: &[ParsedFile] = &[];
        let rs: &[ParsedRustFile] = &[];
        let _ = PyRsTestCoverage::witness();
        let _ = CoverageViolationSpec::witness();
        CheckCoverageGateParams::witness();
        let focus = FocusFilter::unrestricted();
        let _ = CheckCoverageGateParams {
            py_parsed: py,
            rs_parsed: rs,
            gate_config: &gate,
            focus: &focus,
            show_timing: false,
        };
    }
}
