use crate::analyze::FocusFilter;
use kiss::{GateConfig, ParsedFile, ParsedRustFile};

/// Inputs for [`crate::analyze::coverage_gate::check_coverage_gate`].
#[allow(dead_code)]
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

    #[test]
    fn witness_coverage_types() {
        let gate = GateConfig::default();
        let focus = FocusFilter::unrestricted();
        let p = CheckCoverageGateParams {
            py_parsed: &[],
            rs_parsed: &[],
            gate_config: &gate,
            focus: &focus,
            show_timing: false,
        };
        assert!(crate::analyze::check_coverage_gate(&p));
    }
}
