use std::collections::HashSet;
use std::path::PathBuf;

use kiss::check_universe_cache::CachedCoverageItem;
use kiss::cli_output::{CoverageGateFailureCtx, file_coverage_map, print_coverage_gate_failure};

use crate::analyze::coverage::compute_test_coverage_from_lists;
use crate::analyze::coverage_types::CheckCoverageGateParams;
use crate::analyze::focus::is_focus_file;

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

fn per_file_coverage_gate_fails(
    defs_t: &[(PathBuf, String, usize)],
    unrefs_t: &[(PathBuf, String, usize)],
    focus_set: &HashSet<PathBuf>,
    threshold: usize,
) -> Option<PerFileGateFailure> {
    let (_, _, _, unreferenced_focus) =
        compute_test_coverage_from_lists(defs_t, unrefs_t, focus_set);
    let defs_focus: Vec<_> = defs_t
        .iter()
        .filter(|(f, _, _)| is_focus_file(f, focus_set))
        .cloned()
        .collect();
    let file_pcts = file_coverage_map(&defs_focus, &unreferenced_focus);
    if file_pcts.values().any(|&pct| pct < threshold) {
        Some((unreferenced_focus, file_pcts))
    } else {
        None
    }
}

pub(crate) fn evaluate_gate(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
    focus_set: &HashSet<PathBuf>,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let (defs_t, unrefs_t) = analysis_tuples(py_cov, rs_cov);
    if let Some((unreferenced_focus, file_pcts)) =
        per_file_coverage_gate_fails(&defs_t, &unrefs_t, focus_set, threshold)
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

pub(crate) fn evaluate_cached_gate(
    definitions: &[CachedCoverageItem],
    unreferenced: &[CachedCoverageItem],
    focus_set: &HashSet<PathBuf>,
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
        per_file_coverage_gate_fails(&defs, &unrefs, focus_set, threshold)
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
        focus_set,
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
    if let Some((unreferenced, file_pcts)) =
        per_file_coverage_gate_fails(&defs_t, &unrefs_t, focus_set, threshold)
    {
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
mod coverage_gate_tests {
    use super::*;
    use std::collections::HashMap;
    use std::collections::HashSet;

    #[test]
    fn per_file_gate_fails_when_file_below_threshold() {
        use std::path::PathBuf;
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus: HashSet<PathBuf> = std::iter::once(PathBuf::from("src/a.py")).collect();
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90);
        let (unreferenced, file_pcts) = failure.expect("expected gate failure");
        assert_eq!(file_pcts.get(&PathBuf::from("src/a.py")), Some(&0));
        assert_eq!(unreferenced.len(), 1);
    }

    #[test]
    fn per_file_gate_ignores_files_outside_focus() {
        use std::path::PathBuf;
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus: HashSet<PathBuf> = std::iter::once(PathBuf::from("src/b.py")).collect();
        assert!(per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90).is_none());
    }

    #[test]
    fn per_file_gate_passes_when_file_meets_threshold() {
        use std::path::PathBuf;
        let defs = vec![
            (PathBuf::from("src/a.py"), "f".into(), 1),
            (PathBuf::from("src/a.py"), "g".into(), 2),
        ];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus: HashSet<PathBuf> = std::iter::once(PathBuf::from("src/a.py")).collect();
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90);
        let (_, file_pcts) = failure.expect("expected gate failure below 100%");
        assert_eq!(file_pcts.get(&PathBuf::from("src/a.py")), Some(&50));
    }

    #[test]
    fn evaluate_gate_passes_for_empty_analysis() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let focus = HashSet::new();
        assert!(evaluate_gate(&py_cov, &rs_cov, &focus, 90).is_none());
        assert!(evaluate_cached_gate(&[], &[], &focus, 90).is_none());
    }

    #[test]
    fn test_analysis_tuples_empty() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let (defs, unrefs) = analysis_tuples(&py_cov, &rs_cov);
        assert!(defs.is_empty());
        assert!(unrefs.is_empty());
    }
}
