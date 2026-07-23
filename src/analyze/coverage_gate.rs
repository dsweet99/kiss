use std::collections::HashMap;

use crate::analyze::coverage_types::CheckCoverageGateParams;
use crate::analyze::focus::{FocusFilter, is_focus_file};
use crate::analyze::line_coverage::LineCoverageRecord;
use kiss::cli_output::{CoverageGateFailureCtx, print_coverage_gate_failure};

pub(crate) use kiss::cli_output::is_coverage_gate_file;

pub(crate) fn evaluate_line_gate(
    records: &[LineCoverageRecord],
    focus: &FocusFilter,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    if threshold == 0 {
        return None;
    }
    let file_pcts: HashMap<_, _> = records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus))
        .map(|record| (record.file.clone(), record.percent))
        .collect();
    if !file_pcts.values().any(|pct| *pct < threshold) {
        return None;
    }
    let unreferenced = records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus) && record.percent < threshold)
        .map(|record| {
            (
                record.file.clone(),
                "<file>".to_string(),
                record.first_uncovered_line.unwrap_or(1),
            )
        })
        .collect::<Vec<_>>();
    print_coverage_gate_failure(&CoverageGateFailureCtx {
        threshold,
        unreferenced: &unreferenced,
        file_pcts: &file_pcts,
    });
    Some(crate::analyze::options::AnalyzeResult {
        success: false,
        metrics: None,
    })
}

/// Static-reference coverage gating was removed; runtime coverage is owned by `kiss cov`.
#[allow(dead_code)]
pub fn check_coverage_gate(p: &CheckCoverageGateParams<'_>) -> bool {
    let _ = p;
    true
}

#[cfg(test)]
mod inline_coverage_witness {
    use super::*;
    use std::path::Path;

    #[test]
    fn witness_local_is_coverage_gate_file() {
        assert!(is_coverage_gate_file(Path::new("src/lib.rs")));
    }

    #[test]
    fn evaluate_line_gate_threshold_zero_passes() {
        let focus = FocusFilter::unrestricted();
        assert!(evaluate_line_gate(&[], &focus, 0).is_none());
    }

    #[test]
    fn evaluate_line_gate_fails_below_threshold() {
        let focus = FocusFilter::unrestricted();
        let records = vec![LineCoverageRecord {
            file: std::path::PathBuf::from("src/a.py"),
            total_lines: 2,
            covered_lines: 1,
            percent: 50,
            first_uncovered_line: Some(2),
        }];
        assert!(evaluate_line_gate(&records, &focus, 90).is_some());
    }
}
