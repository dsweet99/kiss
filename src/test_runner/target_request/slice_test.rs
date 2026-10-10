use super::projection::SliceProjection;
use super::resolved::{ResolvedTarget, SourceRegion};
use super::slice::{TargetSliceStamp, stamp_from_projection};

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

fn target_slice_stamp(resolved: &ResolvedTarget, complete: bool) -> TargetSliceStamp {
    stamp_from_projection(&projection_from_resolved(resolved), complete)
}

fn projection_from_resolved(resolved: &ResolvedTarget) -> SliceProjection {
    if resolved
        .regions
        .first()
        .is_some_and(|region| matches!(region, SourceRegion::WorkspaceAll))
        && resolved.historical_paths.is_empty()
    {
        return SliceProjection::Workspace {
            selectors: resolved.direct_selectors.clone(),
            sources: Vec::new(),
        };
    }
    if let Some(git) = &resolved.git_stamp {
        return SliceProjection::Vcs {
            git: git.clone(),
            regions: resolved.regions.clone(),
            selectors: resolved.direct_selectors.clone(),
        };
    }
    SliceProjection::SourceRegions {
        regions: resolved.regions.clone(),
        selectors: resolved.direct_selectors.clone(),
    }
}
