use crate::analyze::focus::{FocusFilter, is_focus_file};
use crate::analyze::line_coverage::LineCoverageRecord;
use kiss::cli_output::{coverage_unit_name, format_unreferenced_unit_coverage_message};
use kiss::Violation;

pub(crate) fn collect_line_coverage_viols(
    records: &[LineCoverageRecord],
    focus: &FocusFilter,
    bypass_gate: bool,
) -> Vec<Violation> {
    if !bypass_gate {
        return Vec::new();
    }
    records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus) && record.percent < 100)
        .map(|record| Violation {
            file: record.file.clone(),
            line: record.first_uncovered_line.unwrap_or(1),
            unit_name: coverage_unit_name(&record.file),
            metric: "test_coverage".to_string(),
            value: record.percent,
            threshold: 100,
            message: format_unreferenced_unit_coverage_message(
                record.percent,
                record.covered_lines,
                record.total_lines,
                100,
            ),
            suggestion: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod coverage_line_tests {
    use super::*;
    use crate::analyze::line_coverage::LineCoverageRecord;
    use std::path::PathBuf;

    #[test]
    fn collect_line_coverage_viols_respects_bypass() {
        let records = vec![LineCoverageRecord {
            file: PathBuf::from("src/a.py"),
            total_lines: 2,
            covered_lines: 1,
            percent: 50,
            first_uncovered_line: Some(2),
        }];
        let focus = FocusFilter::unrestricted();
        assert!(collect_line_coverage_viols(&records, &focus, false).is_empty());
        assert_eq!(collect_line_coverage_viols(&records, &focus, true).len(), 1);
    }
}
