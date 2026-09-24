use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::projection::SliceProjection;
use super::resolved::ResolvedTarget;

pub(crate) const TARGET_SLICE_SCHEMA: &str = "target-slice-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetSliceStamp {
    pub digest: String,
    pub complete: bool,
    pub index_schema: String,
}

#[derive(Serialize)]
struct SlicePayload<'a> {
    index_schema: &'a str,
    complete: bool,
    projection: &'a SliceProjection,
}

pub(crate) fn stamp_from_projection(
    projection: &SliceProjection,
    complete: bool,
) -> TargetSliceStamp {
    let payload = SlicePayload {
        index_schema: TARGET_SLICE_SCHEMA,
        complete,
        projection,
    };
    let bytes = serde_json::to_vec(&payload).expect("target slice payload");
    TargetSliceStamp {
        digest: digest_bytes(&bytes),
        complete,
        index_schema: TARGET_SLICE_SCHEMA.to_string(),
    }
}

pub(crate) fn target_slice_stamp(resolved: &ResolvedTarget, complete: bool) -> TargetSliceStamp {
    stamp_from_projection(&projection_from_resolved(resolved), complete)
}

fn projection_from_resolved(resolved: &ResolvedTarget) -> SliceProjection {
    if resolved
        .regions
        .first()
        .is_some_and(|region| matches!(region, super::resolved::SourceRegion::WorkspaceAll))
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
            historical_reverse: resolved
                .historical_paths
                .iter()
                .map(|path| super::resolved::ReverseRecord {
                    path: path.clone(),
                    selectors: Vec::new(),
                })
                .collect(),
            regions: resolved.regions.clone(),
            selectors: resolved.direct_selectors.clone(),
        };
    }
    SliceProjection::SourceRegions {
        regions: resolved.regions.clone(),
        reverse: resolved
            .historical_paths
            .iter()
            .map(|path| super::resolved::ReverseRecord {
                path: path.clone(),
                selectors: Vec::new(),
            })
            .collect(),
    }
}
