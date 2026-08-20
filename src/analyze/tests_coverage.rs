use kiss::GateConfig;

use crate::analyze::CheckCoverageGateParams;
use crate::analyze::FocusFilter;
use crate::analyze::line_coverage::LineCoverageRecord;
use std::path::PathBuf;

#[test]
fn test_static_coverage_gate_is_noop_after_split() {
    let gate = GateConfig {
        test_coverage_threshold: 90,
        ..Default::default()
    };
    let focus = FocusFilter::unrestricted();
    let p = CheckCoverageGateParams {
        py_parsed: &[],
        rs_parsed: &[],
        gate_config: &gate,
        focus: &focus,
        show_timing: false,
    };
    assert!(
        crate::analyze::check_coverage_gate(&p),
        "static-reference coverage gate must not enforce after the kiss cov split"
    );
}

#[test]
fn evaluate_line_gate_reports_below_threshold_files() {
    let focus = FocusFilter::unrestricted();
    let records = vec![LineCoverageRecord {
        file: PathBuf::from("src/poor.py"),
        total_lines: 10,
        covered_lines: 1,
        percent: 10,
        first_uncovered_line: Some(2),
    }];
    assert!(
        crate::analyze::evaluate_line_gate(&records, &focus, 90, kiss::TestCoverageScope::ByFile)
            .is_some(),
        "runtime line gate must fail when focused files are below threshold"
    );
}

#[test]
fn collect_line_coverage_viols_only_when_bypassing_gate() {
    let focus = FocusFilter::unrestricted();
    let records = vec![LineCoverageRecord {
        file: PathBuf::from("src/poor.py"),
        total_lines: 10,
        covered_lines: 1,
        percent: 10,
        first_uncovered_line: Some(2),
    }];
    assert!(crate::analyze::collect_line_coverage_viols(&records, &focus, false).is_empty());
    assert_eq!(
        crate::analyze::collect_line_coverage_viols(&records, &focus, true).len(),
        1
    );
}
