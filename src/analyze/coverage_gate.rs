use std::collections::HashSet;
use std::path::PathBuf;

use kiss::cli_output::{file_coverage_map, print_coverage_gate_failure, CoverageGateFailureCtx};
use kiss::check_universe_cache::CachedCoverageItem;

use crate::analyze::coverage::compute_test_coverage_from_lists;
use crate::analyze::coverage_types::CheckCoverageGateParams;
use crate::analyze::focus::is_focus_file;

type PathNameLine = (PathBuf, String, usize);

fn analysis_tuples(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
) -> (Vec<PathNameLine>, Vec<PathNameLine>) {
    let defs_t: Vec<_> = py_cov
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .chain(
            rs_cov
                .definitions
                .iter()
                .map(|d| (d.file.clone(), d.name.clone(), d.line)),
        )
        .collect();
    let unrefs_t: Vec<_> = py_cov
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .chain(
            rs_cov
                .unreferenced
                .iter()
                .map(|d| (d.file.clone(), d.name.clone(), d.line)),
        )
        .collect();
    (defs_t, unrefs_t)
}

pub(crate) fn evaluate_gate(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
    focus_set: &HashSet<PathBuf>,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let (defs_t, unrefs_t) = analysis_tuples(py_cov, rs_cov);
    let (coverage, _tested, _total, unreferenced_focus) =
        compute_test_coverage_from_lists(&defs_t, &unrefs_t, focus_set);
    if coverage < threshold {
        let file_pcts = file_coverage_map(&defs_t, &unreferenced_focus);
        print_coverage_gate_failure(&CoverageGateFailureCtx {
            threshold,
            unreferenced: &unreferenced_focus,
            file_pcts: &file_pcts,
        });
        return Some(crate::analyze::options::AnalyzeResult {
            success: false,
            metrics: None,
        });
    }
    None
}

#[allow(dead_code)] // Called from unit tests and via `crate::analyze::check_coverage_gate`; not all builds reference it.
pub fn check_coverage_gate(p: &CheckCoverageGateParams<'_>) -> bool {
    let CheckCoverageGateParams {
        py_parsed,
        rs_parsed,
        gate_config,
        focus_set,
        show_timing: _show_timing,
    } = p;
    let (defs_cached, unrefs_cached) = crate::analyze_cache::coverage_lists(py_parsed, rs_parsed);
    let defs_t: Vec<_> = defs_cached.into_iter().map(CachedCoverageItem::into_tuple).collect();
    let unrefs_t: Vec<_> = unrefs_cached.into_iter().map(CachedCoverageItem::into_tuple).collect();
    let (_, _, _, unreferenced) = compute_test_coverage_from_lists(&defs_t, &unrefs_t, focus_set);
    let defs_focus: Vec<_> = defs_t
        .iter()
        .filter(|(f, _, _)| is_focus_file(f, focus_set))
        .cloned()
        .collect();
    let file_pcts = file_coverage_map(&defs_focus, &unreferenced);
    let threshold = gate_config.test_coverage_threshold;
    let any_failing = file_pcts.values().any(|&pct| pct < threshold);
    if any_failing {
        print_coverage_gate_failure(&CoverageGateFailureCtx {
            threshold,
            unreferenced: &unreferenced,
            file_pcts: &file_pcts,
        });
        return false;
    }
    true
}
