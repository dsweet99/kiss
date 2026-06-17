use std::path::PathBuf;

use crate::analyze::FocusFilter;
use kiss::{GateConfig, RustTestRefAnalysis, TestRefAnalysis};

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
    pub py_cov: &'a TestRefAnalysis,
    pub rs_cov: &'a RustTestRefAnalysis,
    pub gate_config: &'a GateConfig,
    pub focus: &'a FocusFilter,
    pub show_timing: bool,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use kiss::GateConfig;
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
        let py_cov = TestRefAnalysis {
            definitions: Vec::new(),
            test_references: Default::default(),
            call_references: Default::default(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: Default::default(),
            call_references: Default::default(),
            propagated_references: Default::default(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let _ = PyRsTestCoverage::witness();
        let _ = CoverageViolationSpec::witness();
        CheckCoverageGateParams::witness();
        let focus = FocusFilter::unrestricted();
        let _ = CheckCoverageGateParams {
            py_cov: &py_cov,
            rs_cov: &rs_cov,
            gate_config: &gate,
            focus: &focus,
            show_timing: false,
        };
    }
}
