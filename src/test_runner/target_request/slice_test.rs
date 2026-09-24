use super::resolved::{ResolvedTarget, SourceRegion};
use super::slice::target_slice_stamp;

#[test]
fn equal_complete_projections_match() {
    let left = ResolvedTarget {
        regions: vec![SourceRegion::FileAll {
            path: "pkg/app.py".into(),
        }],
        direct_selectors: vec!["tests/test_app.py::test_value".into()],
        historical_paths: Vec::new(),
        git_stamp: None,
        operand_classes: Vec::new(),
    };
    let right = left.clone();
    assert_eq!(
        target_slice_stamp(&left, true),
        target_slice_stamp(&right, true)
    );
}

#[test]
fn incomplete_manifest_differs_from_complete() {
    let resolved = ResolvedTarget::workspace();
    assert_ne!(
        target_slice_stamp(&resolved, true),
        target_slice_stamp(&resolved, false)
    );
}

#[test]
fn membership_change_invalidates_slice() {
    let base = ResolvedTarget::workspace();
    let mut extra = base.clone();
    extra
        .direct_selectors
        .push("tests/test_app.py::test_value".into());
    assert_ne!(
        target_slice_stamp(&base, true),
        target_slice_stamp(&extra, true)
    );
}

#[test]
fn historical_path_is_membership_not_coverage() {
    let mut deleted = ResolvedTarget::workspace();
    deleted.regions.clear();
    deleted.historical_paths.push("pkg/gone.py".into());
    let empty = ResolvedTarget {
        regions: Vec::new(),
        ..ResolvedTarget::workspace()
    };
    assert_ne!(
        target_slice_stamp(&deleted, true),
        target_slice_stamp(&empty, true)
    );
}
