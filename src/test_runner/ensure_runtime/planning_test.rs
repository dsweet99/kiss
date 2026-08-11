//! Planning API unit coverage.

use super::{ensure_request_for_selectors, ensure_request_from_planned};
use crate::test_runner::lang_iface::AcceptMode;
use crate::test_runner::PlannedSelectors;
use std::path::PathBuf;

#[test]
fn ensure_request_from_planned_copies_selectors_and_root() {
    let planned = PlannedSelectors {
        repo_root: PathBuf::from("/repo"),
        py_sel: vec!["a".into()],
        rs_sel: vec!["b".into()],
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: vec![],
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: vec![],
        rust_prior_failure_selectors: vec![],
        coverage_decision_engine_used: false,
        rust_selection_basis: crate::test_runner::coverage_decision::RustSelectionBasis::Current,
        ignore: vec!["tmp".into()],
        workspace_files_fingerprint: None,
        skip_python_index_rebuild_after_selective: false,
    };
    let req = ensure_request_from_planned(
        &planned,
        AcceptMode::Subset,
        Some(kiss::Language::Python),
        true,
        4,
        &["-p".into()],
        &["--extra".into()],
        None,
    );
    assert_eq!(req.planned_python, vec!["a".to_string()]);
    assert_eq!(req.planned_rust, vec!["b".to_string()]);
    assert_eq!(req.repo_root, PathBuf::from("/repo"));
    assert!(req.force);
    assert_eq!(req.jobs, 4);
}

#[test]
fn ensure_request_for_selectors_sets_lang_filter() {
    let req = ensure_request_for_selectors(
        PathBuf::from("/r").as_path(),
        &[],
        1,
        kiss::Language::Rust,
        false,
        vec![],
        vec!["t".into()],
    );
    assert_eq!(req.lang_filter, Some(kiss::Language::Rust));
    assert_eq!(req.planned_rust, vec!["t".to_string()]);
}
