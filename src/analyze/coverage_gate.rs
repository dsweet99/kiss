use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

pub(crate) fn is_coverage_gate_file(path: &Path, unit_name: &str) -> bool {
    unit_name != "__test_file__"
        && unit_name != "__entry_point__"
        && !kiss::test_refs::is_test_file(path)
        && !kiss::test_refs::is_in_test_directory(path)
        && !kiss::rust_test_refs::is_rust_test_file(path)
        && !kiss::rust_test_refs::is_binary_entry_point(path)
}

fn is_weighted_overlay_target(path: &Path) -> bool {
    is_coverage_gate_file(path, "")
}

fn overlay_weighted_file_pcts(
    file_pcts: &mut HashMap<PathBuf, usize>,
    unreferenced_focus: &mut Vec<PathNameLine>,
    defs_focus: &[(PathBuf, String, usize)],
    weighted: &HashMap<PathBuf, usize>,
    focus_set: &HashSet<PathBuf>,
) {
    for (path, pct) in weighted {
        if !is_focus_file(path, focus_set) || !is_weighted_overlay_target(path) {
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
    focus_set: &HashSet<PathBuf>,
    threshold: usize,
    weighted_pcts: Option<&HashMap<PathBuf, usize>>,
) -> Option<PerFileGateFailure> {
    let (_, _, _, mut unreferenced_focus) =
        compute_test_coverage_from_lists(defs_t, unrefs_t, focus_set);
    let defs_focus: Vec<_> = defs_t
        .iter()
        .filter(|(f, _, _)| is_focus_file(f, focus_set))
        .cloned()
        .collect();
    let mut file_pcts = file_coverage_map(&defs_focus, &unreferenced_focus);
    if let Some(weighted) = weighted_pcts {
        overlay_weighted_file_pcts(
            &mut file_pcts,
            &mut unreferenced_focus,
            &defs_focus,
            weighted,
            focus_set,
        );
    }
    let gate_fails = file_pcts
        .iter()
        .any(|(path, &pct)| is_coverage_gate_file(path, "") && pct < threshold);
    if gate_fails {
        let file_pcts: HashMap<_, _> = file_pcts
            .into_iter()
            .filter(|(path, _)| is_coverage_gate_file(path, ""))
            .collect();
        let unreferenced_focus: Vec<_> = unreferenced_focus
            .into_iter()
            .filter(|(path, name, _)| is_coverage_gate_file(path, name))
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
    focus_set: &HashSet<PathBuf>,
    threshold: usize,
) -> Option<crate::analyze::options::AnalyzeResult> {
    let (defs_t, unrefs_t) = analysis_tuples(py_cov, rs_cov);
    if let Some((unreferenced_focus, file_pcts)) = per_file_coverage_gate_fails(
        &defs_t,
        &unrefs_t,
        focus_set,
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
        per_file_coverage_gate_fails(&defs, &unrefs, focus_set, threshold, None)
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
    if let Some((unreferenced, file_pcts)) = per_file_coverage_gate_fails(
        &defs_t,
        &unrefs_t,
        focus_set,
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
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None);
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
        assert!(per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None).is_none());
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
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None);
        let (_, file_pcts) = failure.expect("expected gate failure below 100%");
        assert_eq!(file_pcts.get(&PathBuf::from("src/a.py")), Some(&50));
    }

    #[test]
    fn per_file_gate_ignores_test_files_and_sentinels() {
        use std::path::PathBuf;
        let test_py = PathBuf::from("tests/test_foo.py");
        let defs = vec![(test_py.clone(), "__test_file__".into(), 1)];
        let unrefs = vec![(test_py.clone(), "__test_file__".into(), 1)];
        let focus: HashSet<PathBuf> = std::iter::once(test_py).collect();
        assert!(per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None).is_none());
    }

    #[test]
    fn per_file_gate_overlays_weighted_pct_on_binary_zero() {
        use std::path::PathBuf;
        let cliff = PathBuf::from("src/cliffs/cliff_00.rs");
        let defs: Vec<_> = (0..60)
            .map(|i| (cliff.clone(), format!("handler_{i}"), i + 1))
            .chain(std::iter::once((cliff.clone(), "orchestrate".into(), 100)))
            .collect();
        let unrefs: Vec<_> = (0..60)
            .map(|i| (cliff.clone(), format!("handler_{i}"), i + 1))
            .collect();
        let focus: HashSet<PathBuf> = std::iter::once(cliff.clone()).collect();
        let mut weighted = HashMap::new();
        weighted.insert(cliff.clone(), 2);
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, Some(&weighted));
        let (_, file_pcts) = failure.expect("expected gate failure");
        assert_eq!(file_pcts.get(&cliff), Some(&2));
    }

    #[test]
    fn evaluate_gate_passes_for_empty_analysis() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            propagated_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let focus = HashSet::new();
        assert!(evaluate_gate(&py_cov, &rs_cov, &[], &[], &focus, 90).is_none());
        assert!(evaluate_cached_gate(&[], &[], &focus, 90).is_none());
    }

    #[test]
    fn test_analysis_tuples_empty() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            propagated_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let (defs, unrefs) = analysis_tuples(&py_cov, &rs_cov);
        assert!(defs.is_empty());
        assert!(unrefs.is_empty());
    }
}
