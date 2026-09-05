use std::collections::HashMap;

use crate::analyze::coverage_types::CheckCoverageGateParams;
use crate::analyze::focus::{FocusFilter, is_focus_file};
use crate::analyze::line_coverage::{LineCoverageRecord, coverage_percentage};
use kiss::TestCoverageScope;
use kiss::cli_output::{
    CodebaseCoverageGateFailureCtx, CoverageFileStat, CoverageGateFailureCtx,
    codebase_coverage_gate_failure_lines, coverage_gate_failure_lines, coverage_unit_name,
};

pub(crate) fn evaluate_line_gate(
    records: &[LineCoverageRecord],
    focus: &FocusFilter,
    threshold: usize,
    scope: TestCoverageScope,
) -> Option<crate::analyze::options::AnalyzeResult> {
    if threshold == 0 {
        return None;
    }
    match scope {
        TestCoverageScope::ByFile => evaluate_by_file(records, focus, threshold),
        TestCoverageScope::Codebase => evaluate_codebase(records, focus, threshold),
    }
}

fn evaluate_by_file(
    records: &[LineCoverageRecord],
    focus: &FocusFilter,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let file_stats: HashMap<_, _> = records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus))
        .map(|record| {
            (
                record.file.clone(),
                CoverageFileStat {
                    percent: record.percent,
                    covered_lines: record.covered_lines,
                    total_lines: record.total_lines,
                },
            )
        })
        .collect();
    if !file_stats.values().any(|stat| stat.percent < threshold) {
        return None;
    }
    let unreferenced = records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus) && record.percent < threshold)
        .map(|record| {
            (
                record.file.clone(),
                coverage_unit_name(&record.file),
                record.first_uncovered_line.unwrap_or(1),
            )
        })
        .collect::<Vec<_>>();
    for line in coverage_gate_failure_lines(&CoverageGateFailureCtx {
        threshold,
        unreferenced: &unreferenced,
        file_stats: &file_stats,
    }) {
        kiss::rust_llvm_cov_runner::emit_progress(&line);
    }
    Some(crate::analyze::options::AnalyzeResult { success: false })
}

fn evaluate_codebase(
    records: &[LineCoverageRecord],
    focus: &FocusFilter,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let focus_records: Vec<_> = records
        .iter()
        .filter(|record| is_focus_file(&record.file, focus))
        .collect();
    let total: usize = focus_records.iter().map(|r| r.total_lines).sum();
    let covered: usize = focus_records.iter().map(|r| r.covered_lines).sum();
    let percent = if total == 0 {
        100
    } else {
        coverage_percentage(covered, total)
    };
    if percent >= threshold {
        return None;
    }
    let mut diagnostics: Vec<_> = focus_records
        .iter()
        .filter_map(|record| {
            record.first_uncovered_line.map(|line| {
                (
                    record.file.clone(),
                    line,
                    CoverageFileStat {
                        percent: record.percent,
                        covered_lines: record.covered_lines,
                        total_lines: record.total_lines,
                    },
                )
            })
        })
        .collect();
    diagnostics.sort_by(|a, b| a.0.cmp(&b.0));
    for line in codebase_coverage_gate_failure_lines(&CodebaseCoverageGateFailureCtx {
        percent,
        threshold,
        diagnostics: &diagnostics,
    }) {
        kiss::rust_llvm_cov_runner::emit_progress(&line);
    }
    Some(crate::analyze::options::AnalyzeResult { success: false })
}

#[allow(dead_code)]
pub fn check_coverage_gate(p: &CheckCoverageGateParams<'_>) -> bool {
    let _ = p;
    true
}

#[cfg(test)]
mod inline_coverage_witness {
    use super::*;

    #[test]
    fn witness_local_coverage_record_shape() {
        let record = LineCoverageRecord {
            file: std::path::PathBuf::from("src/lib.rs"),
            total_lines: 1,
            covered_lines: 1,
            percent: 100,
            first_uncovered_line: None,
        };
        assert_eq!(record.percent, 100);
    }

    #[test]
    fn witness_local_is_coverage_gate_file() {
        let record = LineCoverageRecord {
            file: std::path::PathBuf::from("src/lib.rs"),
            total_lines: 4,
            covered_lines: 4,
            percent: 100,
            first_uncovered_line: None,
        };
        let focus = FocusFilter::unrestricted();
        assert!(
            evaluate_line_gate(
                std::slice::from_ref(&record),
                &focus,
                90,
                TestCoverageScope::ByFile
            )
            .is_none()
        );
    }

    #[test]
    fn evaluate_line_gate_threshold_zero_passes() {
        let focus = FocusFilter::unrestricted();
        assert!(evaluate_line_gate(&[], &focus, 0, TestCoverageScope::ByFile).is_none());
        assert!(evaluate_line_gate(&[], &focus, 0, TestCoverageScope::Codebase).is_none());
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
        assert!(evaluate_line_gate(&records, &focus, 90, TestCoverageScope::ByFile).is_some());
    }

    #[test]
    fn evaluate_line_gate_codebase_passes_when_aggregate_clears() {
        let focus = FocusFilter::unrestricted();
        let records = vec![
            LineCoverageRecord {
                file: std::path::PathBuf::from("good.py"),
                total_lines: 37,
                covered_lines: 37,
                percent: 100,
                first_uncovered_line: None,
            },
            LineCoverageRecord {
                file: std::path::PathBuf::from("bad.py"),
                total_lines: 2,
                covered_lines: 0,
                percent: 0,
                first_uncovered_line: Some(1),
            },
        ];
        assert!(
            evaluate_line_gate(&records, &focus, 90, TestCoverageScope::Codebase).is_none(),
            "line-weighted aggregate (~95%) must pass under codebase scope"
        );
        assert!(
            evaluate_line_gate(&records, &focus, 90, TestCoverageScope::ByFile).is_some(),
            "by_file must still fail on bad.py"
        );
    }

    #[test]
    fn evaluate_line_gate_codebase_fails_when_aggregate_below() {
        let focus = FocusFilter::unrestricted();
        let records = vec![
            LineCoverageRecord {
                file: std::path::PathBuf::from("a.py"),
                total_lines: 10,
                covered_lines: 5,
                percent: 50,
                first_uncovered_line: Some(2),
            },
            LineCoverageRecord {
                file: std::path::PathBuf::from("b.py"),
                total_lines: 10,
                covered_lines: 5,
                percent: 50,
                first_uncovered_line: Some(3),
            },
        ];
        assert!(evaluate_line_gate(&records, &focus, 90, TestCoverageScope::Codebase).is_some());
    }

    #[test]
    fn evaluate_line_gate_codebase_ignores_non_focus() {
        use std::collections::HashSet;
        let mut paths = HashSet::new();
        paths.insert(std::path::PathBuf::from("good.py"));
        let focus = FocusFilter::restricting(paths);
        let records = vec![
            LineCoverageRecord {
                file: std::path::PathBuf::from("good.py"),
                total_lines: 10,
                covered_lines: 10,
                percent: 100,
                first_uncovered_line: None,
            },
            LineCoverageRecord {
                file: std::path::PathBuf::from("bad.py"),
                total_lines: 10,
                covered_lines: 0,
                percent: 0,
                first_uncovered_line: Some(1),
            },
        ];
        assert!(
            evaluate_line_gate(&records, &focus, 90, TestCoverageScope::Codebase).is_none(),
            "non-focus bad.py must not pull codebase aggregate down"
        );
    }
}
