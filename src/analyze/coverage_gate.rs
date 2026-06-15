use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedCoverageItem;
use kiss::cli_output::{CoverageGateFailureCtx, file_coverage_map, print_coverage_gate_failure};
use crate::analyze::coverage::compute_test_coverage_from_lists;
use crate::analyze::coverage_types::CheckCoverageGateParams;
use crate::analyze::focus::{FocusFilter, is_focus_file};

type PathNameLine = (PathBuf, String, usize);
type PerFileGateFailure = (Vec<PathNameLine>, std::collections::HashMap<PathBuf, usize>);

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

pub(crate) use kiss::cli_output::is_coverage_gate_file;

pub(crate) fn is_coverage_report_target(
    path: &Path,
    unit_name: &str,
    _report_entry_points: bool,
) -> bool {
    let _ = unit_name;
    is_coverage_gate_file(path)
}

fn is_weighted_overlay_target(path: &Path) -> bool {
    is_coverage_gate_file(path)
}

fn overlay_weighted_file_pcts(
    file_pcts: &mut HashMap<PathBuf, usize>,
    unreferenced_focus: &mut Vec<PathNameLine>,
    defs_focus: &[(PathBuf, String, usize)],
    weighted: &HashMap<PathBuf, usize>,
    focus: &FocusFilter,
) {
    for (path, pct) in weighted {
        if !is_focus_file(path, focus) || !is_weighted_overlay_target(path) {
            continue;
        }
        file_pcts.insert(path.clone(), *pct);
        if *pct < 100
            && !unreferenced_focus.iter().any(|(f, _, _)| f == path)
            && let Some(def) = defs_focus.iter().find(|(f, _, _)| f == path)
        {
            unreferenced_focus.push(def.clone());
        }
    }
}

fn per_file_coverage_gate_fails(
    defs_t: &[(PathBuf, String, usize)],
    unrefs_t: &[(PathBuf, String, usize)],
    focus: &FocusFilter,
    threshold: usize,
    weighted_pcts: Option<&HashMap<PathBuf, usize>>,
) -> Option<PerFileGateFailure> {
    let (_, _, _, mut unreferenced_focus) =
        compute_test_coverage_from_lists(defs_t, unrefs_t, focus);
    let defs_focus: Vec<_> = defs_t
        .iter()
        .filter(|(f, _, _)| is_focus_file(f, focus))
        .cloned()
        .collect();
    let mut file_pcts = file_coverage_map(&defs_focus, &unreferenced_focus);
    if let Some(weighted) = weighted_pcts {
        overlay_weighted_file_pcts(
            &mut file_pcts,
            &mut unreferenced_focus,
            &defs_focus,
            weighted,
            focus,
        );
    }
    let gate_fails = file_pcts
        .iter()
        .any(|(path, &pct)| is_coverage_gate_file(path) && pct < threshold);
    if gate_fails {
        let file_pcts: HashMap<_, _> = file_pcts
            .into_iter()
            .filter(|(path, _)| is_coverage_gate_file(path))
            .collect();
        let unreferenced_focus: Vec<_> = unreferenced_focus
            .into_iter()
            .filter(|(path, _name, _)| is_coverage_gate_file(path))
            .collect();
        Some((unreferenced_focus, file_pcts))
    } else {
        None
    }
}

pub(crate) fn evaluate_gate(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
    _py_parsed: &[kiss::ParsedFile],
    _rs_parsed: &[kiss::ParsedRustFile],
    focus: &FocusFilter,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let (defs_t, unrefs_t) = analysis_tuples(py_cov, rs_cov);
    if let Some((unreferenced_focus, file_pcts)) = per_file_coverage_gate_fails(
        &defs_t,
        &unrefs_t,
        focus,
        threshold,
        None,
    ) {
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

pub(crate) fn evaluate_cached_gate(
    definitions: &[CachedCoverageItem],
    unreferenced: &[CachedCoverageItem],
    focus: &FocusFilter,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let defs = definitions
        .iter()
        .map(|item| item.clone().into_tuple())
        .collect::<Vec<_>>();
    let unrefs = unreferenced
        .iter()
        .map(|item| item.clone().into_tuple())
        .collect::<Vec<_>>();
    if threshold == 0 {
        return None;
    }
    if let Some((unreferenced_focus, file_pcts)) =
        per_file_coverage_gate_fails(&defs, &unrefs, focus, threshold, None)
    {
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
        focus,
        show_timing: _show_timing,
    } = p;
    let (defs_cached, unrefs_cached) = crate::analyze_cache::coverage_lists(py_parsed, rs_parsed);
    let defs_t: Vec<_> = defs_cached
        .into_iter()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let unrefs_t: Vec<_> = unrefs_cached
        .into_iter()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let threshold = gate_config.test_coverage_threshold;
    if let Some((unreferenced, file_pcts)) = per_file_coverage_gate_fails(
        &defs_t,
        &unrefs_t,
        focus,
        threshold,
        None,
    ) {
        print_coverage_gate_failure(&CoverageGateFailureCtx {
            threshold,
            unreferenced: &unreferenced,
            file_pcts: &file_pcts,
        });
        return false;
    }
    true
}

#[cfg(test)]
mod inline_coverage_witness {
    use super::*;
    use std::path::Path;

    #[test]
    fn witness_local_is_coverage_gate_file() {
        assert!(is_coverage_gate_file(Path::new("src/lib.rs")));
        assert!(is_coverage_report_target(Path::new("src/lib.rs"), "mod", true));
        assert!(is_weighted_overlay_target(Path::new("src/lib.rs")));
    }
}

#[cfg(test)]
#[path = "coverage_gate_tests.rs"]
mod coverage_gate_tests;
